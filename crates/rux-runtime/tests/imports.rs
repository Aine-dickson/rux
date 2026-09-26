//! Where `use a::b;` looks, and what it says when it finds nothing.
//!
//! Imports resolved **downward only**, relative to the importing file, with no
//! `super::` and no `..`. So a page in `pages/` could not reach a component in
//! `components/` beside its own project root, and the only way out was a second
//! copy of the component. Reported as the thing that catches people out most
//! often.
//!
//! The fallback is root-relative and runs second, so nothing that resolves
//! today can start resolving somewhere else: it can only turn a hard error into
//! a working import.

use std::fs;
use std::path::PathBuf;

use rux_runtime::Document;

/// Distinguishes two scratch directories created inside one clock tick. See the
/// comment where it is used.
static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

const PAGE: &str = "<template><screen><task /></screen></template>\n\
                    <script>\nuse components::task;\n</script>";
const TASK: &str = "<template><view class=\"task\"><text>task</text></view></template>";

/// A throwaway project directory. `files` is `(relative path, contents)`.
fn project(files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rux_imports_{}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        // A counter as well as the clock, because the clock is not enough.
        // Windows ticks its system time about every 15ms, so `as_nanos()` gives
        // two tests that start in the same tick the *same* number; with the same
        // pid that is the same directory, and the first to finish deletes the
        // other's files out from under a load that is still running. It shows up
        // as one unrelated test failing about one run in six, with a different
        // name each time, which is the least debuggable shape a flake has.
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    ));
    for (rel, body) in files {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    dir
}

/// The reported case: a page one directory down, a component at the root.
#[test]
fn a_page_in_a_subdirectory_reaches_a_component_at_the_root() {
    let dir = project(&[
        ("app.rux", "<template><screen><text>app</text></screen></template>"),
        ("components/task.rux", TASK),
        ("pages/home.rux", PAGE),
    ]);
    let doc = Document::load(dir.join("pages/home.rux"));
    let _ = fs::remove_dir_all(&dir);
    assert!(doc.is_ok(), "resolves from the project root: {:?}", doc.err());
}

/// Beside the file still wins, so a component usable from two directories keeps
/// working and nothing silently changes which file it means.
#[test]
fn a_component_beside_the_file_beats_one_at_the_root() {
    let dir = project(&[
        ("app.rux", "<template><screen><text>app</text></screen></template>"),
        // Same import path, two files. The near one must win.
        ("components/task.rux", "<template><view><text>ROOT</text></view></template>"),
        ("pages/components/task.rux", "<template><view><text>BESIDE</text></view></template>"),
        ("pages/home.rux", PAGE),
    ]);
    let doc = Document::load(dir.join("pages/home.rux")).expect("loads");
    let text = format!("{:?}", doc.root);
    let _ = fs::remove_dir_all(&dir);
    assert!(text.contains("BESIDE"), "the near copy is the one used");
    assert!(!text.contains("ROOT"), "and the root copy is not");
}

/// Outside a project there is no root to fall back to, so this is unchanged.
#[test]
fn with_no_entry_point_there_is_no_root_to_fall_back_to() {
    let dir = project(&[("pages/home.rux", PAGE), ("components/task.rux", TASK)]);
    let doc = Document::load(dir.join("pages/home.rux"));
    let _ = fs::remove_dir_all(&dir);
    assert!(doc.is_err(), "no app.rux, so nothing marks the top of a project");
}

/// A component that is nowhere is reported **at the `use` line**, naming every
/// place that was looked.
///
/// It used to be a bare `std::io::Error` with no position at all, so the
/// squiggle was drawn at line 1 and pointed at `<template>` for a mistake on
/// the last line of `<script>`.
#[test]
fn a_missing_component_is_reported_at_its_own_line() {
    let dir = project(&[
        ("app.rux", "<template><screen><text>app</text></screen></template>"),
        ("pages/home.rux", PAGE),
    ]);
    let Err(err) = Document::load_checked(dir.join("pages/home.rux")) else {
        panic!("a component that is nowhere must not load")
    };
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(err.line, Some(3), "the `use` is the third line of the file");
    assert!(err.message.contains("components::task"), "names it: {}", err.message);
    assert!(err.message.contains("looked in"), "and where it looked: {}", err.message);
}

/// A half-typed `use` says so, instead of reporting a filesystem path the
/// author never wrote.
///
/// `use components::;` produced "reading component .../components/.rux: The
/// system cannot find the path specified", which describes a path with an empty
/// filename in it and blames the disk for a half-finished line.
#[test]
fn a_use_with_an_empty_segment_says_so() {
    let dir = project(&[
        ("app.rux", "<template><screen><text>app</text></screen></template>"),
        (
            "pages/home.rux",
            "<template><screen><text>x</text></screen></template>\n\
             <script>\nuse components::;\n</script>",
        ),
    ]);
    let Err(err) = Document::load_checked(dir.join("pages/home.rux")) else {
        panic!("a half-typed `use` must not load")
    };
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(err.line, Some(3), "placed at the `use`");
    assert!(err.message.contains("names no component"), "says what is wrong: {}", err.message);
    assert!(!err.message.contains(".rux:"), "and blames no file: {}", err.message);
}

// -- the two spellings of one name -------------------------------------------
//
// A `use` path is script, where `-` is the subtraction operator, so it has to
// be snake. The file beside it is named by a person, who has been writing
// `<new-task>` all morning. Those two cannot both be right and have the author
// do the reconciling.

/// `use new_task;` finds `new-task.rux`.
///
/// Reported 2026-09-15: the completion list offered `use new-task;` for a file
/// named that way, which resolves only because `use` lines are lifted out of
/// the script before rhai ever sees them. The list now offers the snake
/// spelling, and this is what makes that spelling work.
#[test]
fn a_snake_use_path_finds_a_hyphenated_file() {
    let dir = project(&[(
        "app.rux",
        "<template><screen><new-task /></screen></template>
<script>
use new_task;
</script>",
    ), ("new-task.rux", "<template><view><text>NEW TASK</text></view></template>")]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let text = format!("{:?}", doc.root);
    let _ = fs::remove_dir_all(&dir);
    assert!(text.contains("NEW TASK"), "the hyphenated file is found");
}

/// The exact spelling still wins, so nothing that resolves today moves.
#[test]
fn the_exact_spelling_is_preferred_over_the_hyphenated_one() {
    let dir = project(&[(
        "app.rux",
        "<template><screen><a-b /></screen></template>
<script>
use a_b;
</script>",
    ),
    ("a_b.rux", "<template><view><text>EXACT</text></view></template>"),
    ("a-b.rux", "<template><view><text>HYPHENATED</text></view></template>")]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let text = format!("{:?}", doc.root);
    let _ = fs::remove_dir_all(&dir);
    assert!(text.contains("EXACT"), "the exactly-named file wins");
    assert!(!text.contains("HYPHENATED"), "and the other is not reached");
}

/// A hyphenated `use` path is an error, and names the spelling that works.
///
/// `-` is the minus operator in script, so no name has one; a `use` path is
/// no exception (the project owner's rule, 2026-09-26). It used to be reported
/// and then resolved anyway.
#[test]
fn a_hyphenated_use_path_is_an_error() {
    let dir = project(&[(
        "app.rux",
        "<template><screen><new-task /></screen></template>
<script>
use new-task;
</script>",
    ), ("new-task.rux", "<template><view><text>NEW TASK</text></view></template>")]);
    let err = match Document::load_checked(dir.join("app.rux")) {
        Ok(_) => panic!("a hyphenated path loaded"),
        Err(e) => e,
    };
    let _ = fs::remove_dir_all(&dir);
    let said = err.to_string();
    assert!(said.contains("minus operator"), "{said}");
    assert!(said.contains("use new_task;"), "and names the spelling: {said}");
    assert_eq!(err.line, Some(3), "at the `use` line: {said}");
}

// -- whose imports, and whose tags ------------------------------------------
//
// A `use` is a fact about **one file**. Until 2026-09-15 only the root
// document's were read: a component's own `use` lines were parsed and thrown
// away, so a component could not use a component. The tag matched nothing,
// expanded to nothing, and said nothing.

/// A component may use a component.
///
/// Proven on a real project before it was fixed: `pages/home.rux` carried
/// `use components::task;` and rendered `<task r-for=...>`, `app.rux` imported
/// only the pages, and the list came up empty in a way that read as "no tasks
/// yet".
#[test]
fn a_component_may_use_a_component() {
    let dir = project(&[
        (
            "app.rux",
            "<template><screen><outer /></screen></template>\n<script>\nuse outer;\n</script>",
        ),
        (
            "outer.rux",
            "<template><view><inner /></view></template>\n<script>\nuse inner;\n</script>",
        ),
        ("inner.rux", "<template><text>INNER</text></template>"),
    ]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let text = format!("{:?}", doc.root);
    let _ = fs::remove_dir_all(&dir);
    assert!(text.contains("INNER"), "the nested component renders: {text}");
}

/// A tag is a **local** name: two files may each use their own `<task>`.
///
/// This is the whole reason the registry is keyed by file rather than by tag.
/// Keyed by tag, the second import silently replaced the first and one of the
/// two pages rendered the other's component.
#[test]
fn two_files_may_each_have_their_own_task() {
    let dir = project(&[
        (
            "app.rux",
            "<template><screen><page-a /><page-b /></screen></template>\n\
             <script>\nuse a::page_a;\nuse b::page_b;\n</script>",
        ),
        (
            "a/page_a.rux",
            "<template><view><task /></view></template>\n<script>\nuse a::task;\n</script>",
        ),
        (
            "b/page_b.rux",
            "<template><view><task /></view></template>\n<script>\nuse b::task;\n</script>",
        ),
        ("a/task.rux", "<template><text>A TASK</text></template>"),
        ("b/task.rux", "<template><text>B TASK</text></template>"),
    ]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let text = format!("{:?}", doc.root);
    let _ = fs::remove_dir_all(&dir);
    assert!(text.contains("A TASK"), "the first page gets its own: {text}");
    assert!(text.contains("B TASK"), "and the second gets its own: {text}");
}

/// A tag the document imports is *not* visible inside a component.
///
/// The other half of the same rule, and the one worth pinning: a namespace that
/// leaks is not a namespace, and the flat map this replaced is exactly what
/// leaking looks like.
#[test]
fn a_tag_the_document_imports_is_not_visible_inside_a_component() {
    let dir = project(&[
        (
            "app.rux",
            "<template><screen><outer /><leaf /></screen></template>\n\
             <script>\nuse outer;\nuse leaf;\n</script>",
        ),
        ("outer.rux", "<template><view><leaf /></view></template>"),
        ("leaf.rux", "<template><text>LEAF</text></template>"),
    ]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let said = format!("{:?}", doc.diagnostics());
    let _ = fs::remove_dir_all(&dir);
    assert!(
        said.contains("no element or component called `<leaf>`"),
        "the component has to import it itself: {said}"
    );
}

/// Two files importing each other terminates.
///
/// The worklist checks the registry before walking a file, so the second
/// visit stops there. Recursion would have gone round for ever.
#[test]
fn a_cycle_of_imports_loads_rather_than_hanging() {
    let dir = project(&[
        (
            "app.rux",
            "<template><screen><a-side /></screen></template>\n<script>\nuse a_side;\n</script>",
        ),
        (
            "a_side.rux",
            "<template><view><text>A</text></view></template>\n<script>\nuse b_side;\n</script>",
        ),
        (
            "b_side.rux",
            "<template><view><text>B</text></view></template>\n<script>\nuse a_side;\n</script>",
        ),
    ]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let text = format!("{:?}", doc.root);
    let _ = fs::remove_dir_all(&dir);
    assert!(text.contains("\"A\""), "and still renders: {text}");
}

/// A tag that is neither an element nor imported is an error.
///
/// It can never render anything, which is the test `view=` already uses. It was
/// silent until 2026-09-15, and that silence is what hid the nested-import bug
/// above for the whole of v0.7.
#[test]
fn a_tag_that_names_nothing_is_an_error() {
    let dir = project(&[(
        "app.rux",
        "<template><screen><definitely-not-an-element /></screen></template>",
    )]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let said = format!("{:?}", doc.diagnostics());
    let _ = fs::remove_dir_all(&dir);
    assert!(
        said.contains("no element or component called `<definitely-not-an-element>`"),
        "reported: {said}"
    );
    assert!(said.contains("level: Error"), "as an error: {said}");
}

/// And every element Rux defines is left alone.
#[test]
fn rux_own_elements_are_not_reported_as_unknown() {
    let dir = project(&[(
        "app.rux",
        "<template><screen><view><text>t</text><button><text>b</text></button>\
         <input /><image src=\"x.png\" /><path d=\"M 0 0\" /><slot /></view></screen></template>",
    )]);
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let said = format!("{:?}", doc.diagnostics());
    let _ = fs::remove_dir_all(&dir);
    assert!(!said.contains("no element or component"), "nothing to say: {said}");
}
