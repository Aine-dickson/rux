//! The interpreter, run against the *real* `rux.tmLanguage.json`.
//!
//! The point of this crate is that the playground, VS Code and the site's code
//! fences all colour from one grammar. These tests are what makes that true
//! rather than aspirational: if a grammar edit uses a TextMate feature the
//! interpreter does not implement, loading fails and this test says so — instead
//! of the playground quietly colouring half the file.

use std::path::Path;

use rux_highlight::{to_html, Grammar};

fn grammar() -> Grammar {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../site/syntaxes/rux.tmLanguage.json");
    let json = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    Grammar::from_json(&json).expect("the shipped grammar compiles")
}

const SAMPLE: &str = r#"<template>
  <!-- a comment -->
  <view class="row" r-for="t in items" @tap="n = n + 1">
    <text>{{ t.label }}</text>
  </view>
</template>

<style>
  /* styling */
  .row { display: flex; gap: 8px; color: #cdd6f4; }
</style>

<script>
  let n = signal(0);
  let items = signal([]);
</script>
"#;

/// The invariant everything else depends on: spans tile the source exactly —
/// contiguous, non-overlapping, covering every byte. A renderer that trusted a
/// gappy span list would silently drop source text on screen.
#[test]
fn spans_tile_the_source_exactly() {
    let mut g = grammar();
    let spans = g.spans(SAMPLE);

    assert!(!spans.is_empty(), "no spans produced");
    assert_eq!(spans[0].start, 0, "first span does not start at 0");
    assert_eq!(spans.last().unwrap().end, SAMPLE.len(), "spans stop short of the end");

    for pair in spans.windows(2) {
        assert_eq!(pair[0].end, pair[1].start, "gap or overlap between spans: {pair:?}");
    }
    for s in &spans {
        assert!(s.end > s.start, "empty span: {s:?}");
        assert!(SAMPLE.is_char_boundary(s.start) && SAMPLE.is_char_boundary(s.end),
            "span splits a character: {s:?}");
    }
}

/// Reassembling the spans must reproduce the input byte for byte.
#[test]
fn spans_lose_nothing() {
    let mut g = grammar();
    let rebuilt: String =
        g.spans(SAMPLE).iter().map(|s| &SAMPLE[s.start..s.end]).collect();
    assert_eq!(rebuilt, SAMPLE);
}

/// Spot-check that the classes actually land on the right text. These are the
/// distinctly *Rux* parts — the ones a generic HTML or CSS highlighter would get
/// wrong, and the reason the grammar exists.
#[test]
fn rux_specific_constructs_are_classified() {
    let mut g = grammar();
    let spans = g.spans(SAMPLE);

    let classed = |needle: &str, class: &str| {
        let at = SAMPLE.find(needle).unwrap_or_else(|| panic!("{needle:?} not in sample"));
        spans
            .iter()
            .any(|s| s.start <= at && at < s.end && s.class == Some(class))
    };

    assert!(classed("template", "hl-tag"), "section tag not a tag");
    assert!(classed("r-for", "hl-directive"), "r-for not a directive");
    assert!(classed("@tap", "hl-directive"), "@tap not a directive");
    assert!(classed("<!-- a comment -->", "hl-comment"), "HTML comment not a comment");
    assert!(classed("/* styling */", "hl-comment"), "CSS comment not a comment");
    assert!(classed("signal", "hl-function"), "signal() not a function");
    assert!(classed("\"row\"", "hl-string"), "attribute value not a string");
}

/// A `<style>` block must be coloured as CSS, not as template markup — the
/// begin/end context switch is the part most likely to break.
#[test]
fn style_block_switches_language() {
    let mut g = grammar();
    let spans = g.spans(SAMPLE);
    let at = SAMPLE.find("display").expect("property in sample");
    let class = spans.iter().find(|s| s.start <= at && at < s.end).and_then(|s| s.class);
    assert_eq!(class, Some("hl-property"), "CSS property inside <style> was not recognised");
}

/// The HTML renderer must escape, or a `<view>` in the source would become a
/// real element in the page.
#[test]
fn html_output_is_escaped() {
    let mut g = grammar();
    let html = to_html(&mut g, "<view class=\"a & b\">");
    assert!(!html.contains("<view"), "raw tag survived into the output");
    assert!(html.contains("&lt;"), "angle bracket not escaped");
    assert!(html.contains("&amp;"), "ampersand not escaped");
}

/// Every lesson file must survive the highlighter — they are the code the
/// playground will actually be asked to render.
#[test]
fn every_example_highlights_without_loss() {
    let mut g = grammar();
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut checked = 0;

    for entry in std::fs::read_dir(&dir).expect("examples dir").flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "rux") {
            continue;
        }
        let src = std::fs::read_to_string(&path).expect("read example");
        let rebuilt: String = g.spans(&src).iter().map(|s| &src[s.start..s.end]).collect();
        assert_eq!(rebuilt, src, "{} did not round-trip", path.display());
        checked += 1;
    }
    assert!(checked > 5, "expected to check several examples, saw {checked}");
}
