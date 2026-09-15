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

const PAGE: &str = "<template><screen><task /></screen></template>\n\
                    <script>\nuse components::task;\n</script>";
const TASK: &str = "<template><view class=\"task\"><text>task</text></view></template>";

/// A throwaway project directory. `files` is `(relative path, contents)`.
fn project(files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rux_imports_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
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
