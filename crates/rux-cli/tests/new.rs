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
    for expected in [
        "app.rux",
        "pages/home.rux",
        "pages/missing.rux",
        "stores/greeter.rux",
        "components/message-card.rux",
        "native/Cargo.toml",
        "native/src/lib.rs",
        "native/src/hello.rs",
        "assets/.gitkeep",
        "README.md",
        ".gitignore",
        ".vscode/settings.json",
        "rux.toml",
    ] {
        assert!(root.join(expected).is_file(), "{expected} was not created");
    }

    // The README describes the tree; if it names a directory the scaffold does
    // not create, it is wrong on the reader's first look at it.
    let readme = std::fs::read_to_string(root.join("README.md")).unwrap();
    assert!(readme.contains("my-app"), "the project name did not reach the README");
    for dir in ["components/", "pages/", "stores/", "assets/", "native/", "locks/"] {
        assert!(readme.contains(dir), "the README does not mention {dir}");
    }
    assert!(readme.contains("cargo add"), "the README does not say how a crate is added");
    assert!(readme.contains("--locked"), "the README does not say what CI passes");
}

/// The tasks template is the larger worked example, and is still there.
#[test]
fn the_tasks_template_writes_the_larger_project() {
    let scratch = Scratch::new("tasks-layout");
    let (code, _, stderr) = run(scratch.path(), &["new", "my-app", "--template", "tasks"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    let root = scratch.path().join("my-app");
    for expected in [
        "app.rux",
        "stores/todo.rux",
        "pages/home.rux",
        "pages/new-task.rux",
        "pages/detail.rux",
        "pages/missing.rux",
        "components/task-row.rux",
        "native/Cargo.toml",
    ] {
        assert!(root.join(expected).is_file(), "{expected} was not created");
    }
    let readme = std::fs::read_to_string(root.join("README.md")).unwrap();
    assert!(readme.contains("stores/todo.rux"));
    assert!(readme.contains("rux check components/task-row.rux"), "{readme}");
}

/// The Rust half is on unless it is asked off, and off leaves no trace of it.
#[test]
fn native_is_on_by_default_and_off_leaves_nothing_behind() {
    let scratch = Scratch::new("no-native");
    let (code, _, stderr) = run(scratch.path(), &["new", "plain", "--no-native"]);
    assert_eq!(code, 0, "stderr: {stderr}");
    let root = scratch.path().join("plain");
    assert!(!root.join("native").exists(), "native/ was written");
    assert!(!root.join(".vscode").exists(), "the rust-analyzer settings were written");
    let readme = std::fs::read_to_string(root.join("README.md")).unwrap();
    assert!(!readme.contains("native/"), "the README describes a directory that is not there");
    assert!(!readme.contains("cargo add"), "{readme}");
    // And what is left is a project that still checks.
    let (code, stdout, stderr) = run(&root, &["check"]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
}

/// Without Rust the minimal project still shows something real: the greeting is
/// computed in the page, not left as an empty card.
#[test]
fn the_minimal_project_without_native_renders_its_greeting() {
    fn texts(node: &rux_layout::Node) -> Vec<String> {
        let mut out: Vec<String> = node.text.iter().map(|t| t.text.trim().to_string()).collect();
        for child in &node.children {
            out.extend(texts(child));
        }
        out
    }
    let scratch = Scratch::new("greeting");
    assert_eq!(run(scratch.path(), &["new", "app", "--no-native"]).0, 0);
    let doc = rux_runtime::Document::load(scratch.path().join("app/app.rux")).expect("loads");
    let shown = texts(&doc.root);
    assert!(shown.contains(&"Hello, World!".to_string()), "{shown:?}");
    assert!(shown.contains(&"app".to_string()), "the project name is not the header: {shown:?}");
}

/// The native crate is read by Rux the way a build would read it, and its
/// dependency is the version of Rux that wrote it.
#[test]
fn the_scaffolded_native_crate_is_the_one_rux_sees() {
    let scratch = Scratch::new("native-crate");
    assert_eq!(run(scratch.path(), &["new", "app"]).0, 0);
    let root = scratch.path().join("app");

    let (code, stdout, stderr) = run(&root, &["native"]);
    assert_eq!(code, 0, "{stdout}\n{stderr}");
    assert!(stdout.contains("greet"), "Rux does not see the export: {stdout}");

    let cargo = std::fs::read_to_string(root.join("native/Cargo.toml")).unwrap();
    assert!(
        cargo.contains(&format!("version = \"{}\"", env!("CARGO_PKG_VERSION"))),
        "the native crate does not depend on this Rux: {cargo}"
    );
    assert!(!cargo.contains("%%"), "a placeholder reached the file: {cargo}");
}

/// **The scaffold fills in its own placeholders and touches nothing else.** The
/// first version replaced every `{name}` in every file, which rewrote the Rust's
/// own `format!("Hello, {name}!")` into `format!("Hello, demo!")`: it compiled,
/// it checked clean, and it greeted everyone by the project's name. Only typing
/// into the window showed it.
#[test]
fn the_scaffold_leaves_the_authors_own_braces_alone() {
    fn files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                files(&path, out);
            } else {
                out.push(path);
            }
        }
    }
    let scratch = Scratch::new("braces");
    assert_eq!(run(scratch.path(), &["new", "demo"]).0, 0);
    let root = scratch.path().join("demo");

    let rust = std::fs::read_to_string(root.join("native/src/hello.rs")).unwrap();
    assert!(rust.contains(r#"format!("Hello, {name}!")"#), "the Rust was rewritten:\n{rust}");

    let mut all = Vec::new();
    files(&root, &mut all);
    for path in all {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(!text.contains("%%"), "a placeholder reached {}", path.display());
    }
    let app = std::fs::read_to_string(root.join("app.rux")).unwrap();
    assert!(app.contains(r#"<text class="brand">demo</text>"#), "the name did not reach the header:\n{app}");
}

/// A build writes `dist/`, `.rux-build/` and `target/`, and the kept locks are
/// the one thing there that should be committed.
#[test]
fn the_gitignore_names_what_a_build_writes_and_not_the_locks() {
    let scratch = Scratch::new("gitignore");
    assert_eq!(run(scratch.path(), &["new", "app"]).0, 0);
    let ignore = std::fs::read_to_string(scratch.path().join("app/.gitignore")).unwrap();
    let rules: Vec<&str> = ignore.lines().map(str::trim).filter(|l| !l.starts_with('#')).collect();
    for ignored in ["dist/", ".rux-build/", "target/"] {
        assert!(rules.contains(&ignored), "{ignored} is not ignored");
    }
    assert!(!rules.iter().any(|r| r.contains("locks")), "the locks are ignored");
}

#[test]
fn an_unknown_template_is_refused_and_named() {
    let scratch = Scratch::new("bad-template");
    let (code, _, stderr) = run(scratch.path(), &["new", "app", "--template", "banana"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("`banana`") && stderr.contains("minimal"), "stderr: {stderr}");
}

/// The whole point of scaffolding: what comes out runs.
#[test]
fn what_the_scaffold_writes_checks_clean() {
    let scratch = Scratch::new("check");
    for (dir, args, component) in [
        ("minimal", vec!["new", "minimal"], "components/message-card.rux"),
        ("tasks", vec!["new", "tasks", "--template", "tasks"], "components/task-row.rux"),
    ] {
        assert_eq!(run(scratch.path(), &args).0, 0);
        let root = scratch.path().join(dir);

        let (code, stdout, stderr) = run(&root, &["check"]);
        assert_eq!(code, 0, "a fresh {dir} project does not check clean\n{stdout}\n{stderr}");

        // The bare check above already read the component on its own, and said
        // so: a broken one is found without naming it.
        assert!(stderr.contains("1 component"), "the walk covers the component {component}
{stderr}");
        let (code, stdout, stderr) = run(&root, &["check", component]);
        assert_eq!(code, 0, "the {dir} component does not check clean
{stdout}
{stderr}");
    }
}

/// The detail page reads its task through a `computed`, which has to follow
/// both the route and the store.
#[test]
fn the_scaffolds_detail_page_follows_its_task() {
    fn texts(node: &rux_layout::Node) -> Vec<String> {
        let mut out: Vec<String> = node.text.iter().map(|t| t.text.trim().to_string()).collect();
        for child in &node.children {
            out.extend(texts(child));
        }
        out
    }
    fn tappable<'a>(node: &'a rux_layout::Node, label: &str) -> Option<&'a rux_layout::Node> {
        if node.on_tap.is_some() && texts(node).iter().any(|t| t == label) {
            return Some(node);
        }
        node.children.iter().find_map(|c| tappable(c, label))
    }
    let scratch = Scratch::new("detail");
    assert_eq!(run(scratch.path(), &["new", "app", "--template", "tasks"]).0, 0);
    let mut doc = rux_runtime::Document::load(scratch.path().join("app/app.rux")).expect("loads");
    doc.navigate("/task/2");
    assert!(texts(&doc.root).contains(&"Add a task of your own".to_string()), "{:?}", texts(&doc.root));
    let button = tappable(&doc.root, "Mark as done").expect("an undone task offers done").clone();
    assert!(doc.apply_handler_in(&button.on_tap.clone().unwrap(), button.instance.as_deref()));
    assert!(tappable(&doc.root, "Mark as not done").is_some(), "{:?}", texts(&doc.root));
    doc.navigate("/task/1");
    assert!(texts(&doc.root).contains(&"Read the Rux guide".to_string()), "{:?}", texts(&doc.root));
    doc.navigate("/task/9");
    assert!(texts(&doc.root).contains(&"That task is no longer here.".to_string()), "{:?}", texts(&doc.root));
}

/// And what comes out is already formatted. This caught a real mistake: the
/// `<script>` body was written at column 0 while `rux fmt` indents it one level,
/// so the first `rux fmt` in a new project rewrote the file it had just made.
#[test]
fn what_the_scaffold_writes_is_already_formatted() {
    let scratch = Scratch::new("fmt");
    for (dir, args) in [
        ("minimal", vec!["new", "minimal"]),
        ("tasks", vec!["new", "tasks", "--template", "tasks"]),
    ] {
        assert_eq!(run(scratch.path(), &args).0, 0);
        let root = scratch.path().join(dir);

        let (code, stdout, stderr) = run(&root, &["fmt", "--check", "."]);
        assert_eq!(
            code, 0,
            "the {dir} scaffold is not formatted the way `rux fmt` would write it\n{stdout}\n{stderr}"
        );
    }
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

/// A directory where a document was expected is the third shape of the same
/// crash: the runtime starts a file watcher on the path and panics.
#[test]
fn a_directory_is_not_a_document() {
    let scratch = Scratch::new("dir");
    std::fs::create_dir_all(scratch.path().join("somewhere")).unwrap();
    let (code, _, stderr) = run(scratch.path(), &["run", "somewhere"]);

    assert_eq!(code, 2);
    assert!(!stderr.contains("panicked"), "still panicking: {stderr}");
    assert!(stderr.contains("is a directory"), "stderr: {stderr}");
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

#[test]
fn a_scaffolded_project_can_be_built_without_writing_a_file_by_hand() {
    // The gap this closes: `rux new` wrote no manifest, so `rux build` on a
    // fresh project stopped at "no rux.toml" and the author had to write one
    // before the tool they had just been told to run would do anything.
    let scratch = Scratch::new("manifest");
    let (code, _, stderr) = run(scratch.path(), &["new", "my-app"]);
    assert_eq!(code, 0, "stderr: {stderr}");

    let manifest = std::fs::read_to_string(scratch.path().join("my-app").join("rux.toml"))
        .expect("the scaffold wrote no rux.toml");
    assert!(manifest.contains(r#"name = "my-app""#), "{manifest}");
    assert!(manifest.contains("version ="), "{manifest}");
    // `entry` is deliberately absent: `app.rux` is what the lookup prefers
    // anyway, and writing it turns a convention into configuration.
    assert!(!manifest.contains("entry ="), "{manifest}");
}

#[test]
fn a_scaffolded_id_is_a_package_name_android_will_accept() {
    // **A dash is legal in a project name and illegal in an Android package
    // segment.** `dev.example.my-app` passes our own manifest check, which only
    // looks for dots, and is rejected by `aapt2` at the end of a full Android
    // build. The scaffold is where that is cheapest to get right.
    let scratch = Scratch::new("package-id");
    let (code, _, stderr) = run(scratch.path(), &["new", "my-app"]);
    assert_eq!(code, 0, "stderr: {stderr}");

    let manifest = std::fs::read_to_string(scratch.path().join("my-app").join("rux.toml"))
        .expect("reading the manifest");
    let id = manifest
        .lines()
        .find_map(|l| l.trim().strip_prefix("id = "))
        .expect("no id")
        .trim_matches('"')
        .to_string();
    assert!(!id.contains('-'), "a dash reached the package name: {id}");
    for segment in id.split('.') {
        assert!(!segment.is_empty(), "empty segment in {id}");
        assert!(
            !segment.starts_with(|c: char| c.is_ascii_digit()),
            "segment starts with a digit in {id}"
        );
        assert!(
            segment.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "not a Java identifier: {segment} in {id}"
        );
    }
}
