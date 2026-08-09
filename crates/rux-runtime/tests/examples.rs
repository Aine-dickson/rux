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

/// Drive the computed/effect example the way a person would, and check the
/// numbers it puts on screen.
///
/// The suite already proves every example *loads*; this proves one of them
/// *works*, which for a reactivity feature is the part that can quietly rot.
#[test]
fn the_computed_example_recomputes_when_tapped() {
    fn texts(node: &rux_layout::Node) -> Vec<String> {
        let mut out: Vec<String> = node.text.iter().map(|t| t.text.clone()).collect();
        for child in &node.children {
            out.extend(texts(child));
        }
        out
    }
    let has = |doc: &Document, needle: &str| texts(&doc.root).iter().any(|t| t == needle);

    let mut doc = Document::load(examples_dir().join("computed.rux")).expect("loads");
    // qty 2 x price 12 → 24, tax 2.4, total 26.4, and the effect has run once.
    assert!(has(&doc, "24"), "subtotal on load: {:?}", texts(&doc.root));
    assert!(has(&doc, "26.4"), "total on load: {:?}", texts(&doc.root));
    assert!(
        texts(&doc.root).iter().any(|t| t.contains("within budget")),
        "the effect ran on load, so the status is not blank: {:?}",
        texts(&doc.root)
    );

    // Tap `+` eight times: 10 x 12 = 120, over the 100 budget.
    for _ in 0..8 {
        assert!(doc.apply_handler("qty = qty + 1"), "the tap changed state");
    }
    assert!(has(&doc, "120"), "subtotal followed: {:?}", texts(&doc.root));
    assert!(has(&doc, "132"), "and so did the computed that reads it");
    assert!(
        texts(&doc.root).iter().any(|t| t.contains("over budget")),
        "the effect re-ran and flipped the status: {:?}",
        texts(&doc.root)
    );
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
            let listed: Vec<String> = warnings.iter().map(|w| w.to_string()).collect();
            noisy.push(format!("{}:\n  - {}", path.display(), listed.join("\n  - ")));
        }
    }
    assert!(
        noisy.is_empty(),
        "examples raise warnings the dev overlay will show:\n{}",
        noisy.join("\n")
    );
}
