//! Every shipped example must load, and load *clean*.
//!
//! Now that the dev overlay shows warnings in the window, a noisy example is a
//! visible defect: open it and a panel covers the demo. This walks `examples/`
//! and fails with the offending file and message, so the examples stay the
//! reference for what good `.rux` looks like.

use std::path::{Path, PathBuf};

use rux_runtime::Document;

fn examples_dir() -> PathBuf {
    // Tests run with the crate root as the working directory.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn example_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(examples_dir())
        .expect("examples/ is readable")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension()? == "rux").then_some(path)
        })
        .collect();
    files.sort();
    assert!(!files.is_empty(), "found no examples to check");
    files
}

#[test]
fn every_example_loads() {
    let mut failures = Vec::new();
    for path in example_files() {
        if let Err(err) = Document::load(&path) {
            failures.push(format!("{}: {err}", path.display()));
        }
    }
    assert!(failures.is_empty(), "examples failed to load:\n{}", failures.join("\n"));
}

#[test]
fn every_example_is_warning_free() {
    let mut noisy = Vec::new();
    for path in example_files() {
        let Ok(doc) = Document::load(&path) else { continue }; // reported by the test above
        let warnings = &doc.diagnostics().warnings;
        if !warnings.is_empty() {
            noisy.push(format!("{}:\n  - {}", path.display(), warnings.join("\n  - ")));
        }
    }
    assert!(
        noisy.is_empty(),
        "examples raise warnings the dev overlay will show:\n{}",
        noisy.join("\n")
    );
}
