//! `rux new`, and the workspace entry lookup it pairs with.
//!
//! The scaffold is documentation that happens to be executable, so the tests
//! that matter are the ones a reader would perform: does it run, is it clean,
//! and does `rux` find it. A scaffold that produced a file `rux check` rejects,
//! or one `rux fmt` immediately rewrites, would teach the wrong thing on the
//! first minute of contact.

use std::path::{Path, PathBuf};
use std::process::Command;

fn rux() -> PathBuf {
    // The integration-test binary sits beside the one under test.
    let mut path = std::env::current_exe().expect("test binary path");
    path.pop();
    if path.ends_with("deps") {
        path.pop();
    }
    path.join(format!("rux{}", std::env::consts::EXE_SUFFIX))
}

/// A scratch directory that cleans itself up, named after the calling test so a
/// failure leaves something identifiable behind.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("rux-new-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Scratch(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(cwd: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(rux()).args(args).current_dir(cwd).output().expect("running rux");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn the_scaffold_writes_the_layout_it_documents() {
    let scratch = Scratch::new("layout");
    let (code, stdout, stderr) = run(scratch.path(), &["new", "my-app"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    assert!(stdout.contains("cd my-app"), "the next step is not spelled out");
    assert!(stdout.contains("rux run"), "the run command is not spelled out");

    let root = scratch.path().join("my-app");
    for expected in
        ["app.rux", "components/task.rux", "assets/README.md", "README.md", ".gitignore"]
    {
        assert!(root.join(expected).is_file(), "{expected} was not created");
    }

    // The README describes the tree; if it names a directory the scaffold does
    // not create, it is wrong on the reader's first look at it.
    let readme = std::fs::read_to_string(root.join("README.md")).unwrap();
    assert!(readme.contains("my-app"), "the project name did not reach the README");
    assert!(readme.contains("components/"));
    assert!(readme.contains("assets/"));
}

/// The whole point of scaffolding: what comes out runs.
#[test]
fn what_the_scaffold_writes_checks_clean() {
    let scratch = Scratch::new("check");
    assert_eq!(run(scratch.path(), &["new", "app"]).0, 0);
    let root = scratch.path().join("app");

    let (code, stdout, stderr) = run(&root, &["check"]);
    assert_eq!(code, 0, "a fresh project does not check clean\n{stdout}\n{stderr}");

    // Components are skipped when walking, so the one that is there is named
    // explicitly. A broken component would otherwise go unnoticed until run.
    let (code, stdout, stderr) = run(&root, &["check", "components/task.rux"]);
    assert_eq!(code, 0, "the scaffolded component does not check clean\n{stdout}\n{stderr}");
}

/// And what comes out is already formatted. This caught a real mistake: the
/// `<script>` body was written at column 0 while `rux fmt` indents it one level,
/// so the first `rux fmt` in a new project rewrote the file it had just made.
#[test]
fn what_the_scaffold_writes_is_already_formatted() {
    let scratch = Scratch::new("fmt");
    assert_eq!(run(scratch.path(), &["new", "app"]).0, 0);
    let root = scratch.path().join("app");

    let (code, stdout, stderr) = run(&root, &["fmt", "--check", "."]);
    assert_eq!(
        code, 0,
        "the scaffold is not formatted the way `rux fmt` would write it\n{stdout}\n{stderr}"
    );
}

#[test]
fn a_second_scaffold_refuses_rather_than_overwrites() {
    let scratch = Scratch::new("occupied");
    assert_eq!(run(scratch.path(), &["new", "app"]).0, 0);

    let (code, _, stderr) = run(scratch.path(), &["new", "app"]);
    assert_eq!(code, 2, "an occupied directory was scaffolded into");
    assert!(stderr.contains("already exists"), "stderr: {stderr}");

    // And the original survived.
    assert!(scratch.path().join("app/app.rux").is_file());
}

#[test]
fn a_name_that_would_bite_later_is_refused() {
    let scratch = Scratch::new("names");
    let (code, _, stderr) = run(scratch.path(), &["new", "my app"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("use letters"), "stderr: {stderr}");

    let (code, _, stderr) = run(scratch.path(), &["new"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("needs a name"), "stderr: {stderr}");
}

/// `rux` from a subdirectory has to find the project, the way `git` does. Being
/// told to `cd` back to the root is a papercut in a command run dozens of times
/// an hour.
///
/// The window cannot be opened in a test, so this asserts on what `run` does
/// *before* it opens one: a resolved entry gets past the existence check, and a
/// missing one does not.
#[test]
fn the_entry_point_is_found_from_a_subdirectory() {
    let scratch = Scratch::new("walkup");
    assert_eq!(run(scratch.path(), &["new", "app"]).0, 0);
    let nested = scratch.path().join("app/components");

    // `check` with no path walks the current directory, so it proves nothing
    // about the lookup. `run` is what resolves an entry, and the failure mode
    // being tested for is the message, not the window.
    let (_, _, stderr) = run(&nested, &["run", "--route"]);
    assert!(
        !stderr.contains("no app.rux"),
        "the entry point was not found from a subdirectory: {stderr}"
    );
    assert!(stderr.contains("--route"), "expected to reach argument parsing: {stderr}");
}

/// Bare `rux` says what `rux` is, the way `cargo` and `git` do. It used to
/// launch a GUI, and specifically to launch `examples/battery.rux`, a path that
/// exists only in a checkout of this repo: for everyone else the first thing
/// typed after `cargo install ruxlang` was a panic out of the file watcher.
#[test]
fn bare_rux_prints_the_usage() {
    let scratch = Scratch::new("bare");
    let (code, stdout, stderr) = run(scratch.path(), &[]);
    assert_eq!(code, 0, "bare `rux` should not be an error: {stderr}");
    assert!(stdout.contains("Usage:"), "no usage printed");
    assert!(stdout.contains("rux run"), "the run command is not listed");
    assert!(stdout.contains("rux new"), "the new command is not listed");
    assert!(!stderr.contains("panicked"), "still panicking: {stderr}");
}

/// `rux run` inside a project with no entry point still has to explain itself.
#[test]
fn nothing_to_run_explains_itself_instead_of_panicking() {
    let scratch = Scratch::new("empty");
    let (code, _, stderr) = run(scratch.path(), &["run"]);

    assert_eq!(code, 2, "expected a usage error");
    assert!(!stderr.contains("panicked"), "still panicking: {stderr}");
    assert!(stderr.contains("app.rux or index.rux"), "does not say what it looked for");
    assert!(stderr.contains("rux new"), "does not offer the way out");
    assert!(stderr.contains("Usage:"), "the commands are not listed");
}

/// A named file that is not there is an ordinary mistake and must not read like
/// a bug in Rux.
#[test]
fn a_missing_file_is_a_message_not_a_stack_trace() {
    let scratch = Scratch::new("missing");
    let (code, _, stderr) = run(scratch.path(), &["run", "nope.rux"]);
    assert_eq!(code, 2);
    assert!(!stderr.contains("panicked"), "still panicking: {stderr}");
    assert!(stderr.contains("does not exist"), "stderr: {stderr}");
}

/// Every subcommand the usage lists has a page under `/tooling/`.
///
/// The docs problem this whole section exists to fix was not that pages were
/// wrong, it was that there was nowhere to look: one 1173-line reference and a
/// four-entry sidebar. A subcommand added later without a page would quietly
/// recreate that, one command at a time.
#[test]
fn every_subcommand_has_a_tooling_page() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tooling = root.join("site/content/tooling");
    assert!(tooling.is_dir(), "the tooling section is gone");

    let (_, usage, _) = run(&root, &["--help"]);
    // `rux new`, `rux run`, `rux check`, `rux fmt`, `rux vocab`: the words that
    // follow `rux ` at the start of a usage line, deduplicated.
    let mut commands: Vec<&str> = usage
        .lines()
        .filter_map(|l| l.trim().strip_prefix("rux "))
        .filter_map(|rest| rest.split_whitespace().next())
        .filter(|w| w.chars().all(|c| c.is_ascii_lowercase()))
        .collect();
    commands.sort_unstable();
    commands.dedup();
    assert!(commands.len() >= 5, "usage parsed to too few commands: {commands:?}");

    for command in commands {
        let page = tooling.join(format!("{command}.md"));
        assert!(
            page.is_file(),
            "`rux {command}` is in the usage but has no page at site/content/tooling/{command}.md"
        );
    }
}

#[test]
fn index_rux_is_accepted_as_an_entry_point() {
    let scratch = Scratch::new("index");
    std::fs::write(
        scratch.path().join("index.rux"),
        "<template>\n  <screen>\n    <text>hi</text>\n  </screen>\n</template>\n",
    )
    .unwrap();

    // Reaching argument parsing means the entry resolved; the window is never
    // opened because `--route` is rejected first.
    let (_, _, stderr) = run(scratch.path(), &["run", "--route"]);
    assert!(!stderr.contains("no app.rux"), "index.rux was not accepted: {stderr}");
}
