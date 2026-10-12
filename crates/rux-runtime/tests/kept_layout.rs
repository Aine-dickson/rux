//! Every example, laid out through a kept Taffy tree (`rux_layout::LayoutState`,
//! track (g).4), is laid out as a whole layout would lay it out.
//!
//! The random trees of rux-layout's `tests/kept.rs` use a handful of styles;
//! the examples use the rest: grid, scrollers, sticky, transforms, paths,
//! images, fields, static-position boxes. Each is taken through the window
//! sizes a person gives it (the same size twice, narrower, a phone, back) and
//! every frame is compared whole, paints, regions and metrics.

use std::path::PathBuf;

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

/// Greedy word wrap, so a stale width or cap shows as a wrong height.
fn measure(tc: &rux_layout::TextContent, max: Option<f32>) -> (f32, f32) {
    let char_w = tc.font_size * 0.5;
    let line_h = tc.line_height.unwrap_or(tc.font_size * 1.25);
    let width = |s: &str| s.chars().count() as f32 * char_w;
    let Some(max) = max.filter(|_| !tc.nowrap) else {
        return (width(&tc.text), line_h);
    };
    let (mut lines, mut line, mut widest) = (1.0, 0.0f32, 0.0f32);
    for word in tc.text.split(' ') {
        let w = width(word);
        let next = if line == 0.0 { w } else { line + char_w + w };
        if next > max && line > 0.0 {
            lines += 1.0;
            widest = widest.max(line);
            line = w;
        } else {
            line = next;
        }
    }
    (widest.max(line), lines * line_h)
}

#[test]
fn every_example_lays_out_kept_as_whole() {
    let mut files: Vec<PathBuf> = std::fs::read_dir(examples_dir())
        .expect("examples")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "rux"))
        .collect();
    files.sort();
    let sizes = [(1000.0, 800.0), (1000.0, 800.0), (640.0, 800.0), (360.0, 640.0), (360.0, 640.0), (1000.0, 800.0)];
    let mut checked = 0;
    for file in files {
        let name = file.file_stem().unwrap().to_string_lossy().to_string();
        let Ok(doc) = rux_runtime::Document::load(&file) else {
            continue;
        };
        let mut kept = rux_layout::LayoutState::new();
        for (i, (w, h)) in sizes.iter().enumerate() {
            let out = kept.layout_scrolled(&doc.root, *w, *h, &[], &mut measure);
            let whole = rux_layout::layout(&doc.root, *w, *h, &mut measure);
            assert_eq!(
                format!("{out:?}"),
                format!("{whole:?}"),
                "{name}, frame {i} at {w}x{h}: the kept layout differs from the whole one"
            );
            checked += 1;
        }
    }
    assert!(checked > 200, "only {checked} frames checked");
}
