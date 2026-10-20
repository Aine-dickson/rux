//! A paragraph made of runs and inline boxes: `Read the <text>terms</text>`,
//! an icon in a sentence. One layout, so it measures, breaks and places as one
//! text, with each line as tall as what is on it.

use rux_layout::{InlineItem, Rgba, RichText, RunStyle, TextRun, VerticalAlign};
use rux_text::{Align, TextEngine, TextStyle, Wrap};

const SIZE: f32 = 16.0;

fn base() -> TextStyle<'static> {
    TextStyle::new(SIZE, 400, Wrap::Normal)
}

fn run(range: std::ops::Range<usize>, font_size: f32, weight: u16) -> TextRun {
    TextRun {
        range,
        style: RunStyle {
            font_size,
            weight,
            italic: false,
            family: None,
            letter_spacing: None,
            word_spacing: None,
            underline: false,
            strikethrough: false,
            color: Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 },
            background: None,
            look: None,
        },
    }
}

fn item(index: usize, width: f32, height: f32, align: VerticalAlign) -> InlineItem {
    InlineItem { index, width, height, align }
}

/// A run styled exactly as the paragraph changes nothing: the paragraph
/// measures as the plain text does.
#[test]
fn a_run_in_the_paragraphs_own_style_measures_as_plain_text() {
    let mut te = TextEngine::new();
    let text = "Read the terms before you sign.";
    let plain = te.measure(text, &base(), None);
    let rich = RichText { runs: vec![run(9..14, SIZE, 400)], items: vec![] };
    let style = TextStyle { rich: Some(&rich), ..base() };
    assert_eq!(te.measure(text, &style, None), plain);
}

/// A larger run makes its line taller, and only its line.
#[test]
fn a_larger_run_makes_its_line_taller() {
    let mut te = TextEngine::new();
    let text = "small BIG small";
    let (_, plain_h) = te.measure(text, &base(), None);
    let rich = RichText { runs: vec![run(6..9, 32.0, 400)], items: vec![] };
    let style = TextStyle { rich: Some(&rich), ..base() };
    let (_, h) = te.measure(text, &style, None);
    assert!(h > plain_h * 1.5, "{h} against {plain_h}");
}

/// The same text with different runs is a different paragraph, cache or not.
#[test]
fn different_runs_are_not_answered_from_the_cache() {
    let mut te = TextEngine::new();
    let text = "small BIG small";
    let a = RichText { runs: vec![run(6..9, 32.0, 400)], items: vec![] };
    let b = RichText { runs: vec![run(6..9, 48.0, 400)], items: vec![] };
    let ha = te.measure(text, &TextStyle { rich: Some(&a), ..base() }, None).1;
    let hb = te.measure(text, &TextStyle { rich: Some(&b), ..base() }, None).1;
    assert!(hb > ha, "{hb} against {ha}");
}

/// A box takes its width in the line, and the words after it move along.
#[test]
fn an_inline_box_takes_its_width_in_the_line() {
    let mut te = TextEngine::new();
    let text = "Tap  to open";
    let (plain_w, plain_h) = te.measure(text, &base(), None);
    let rich = RichText { runs: vec![], items: vec![item(4, 30.0, 10.0, VerticalAlign::Middle)] };
    let style = TextStyle { rich: Some(&rich), ..base() };
    let (w, h) = te.measure(text, &style, None);
    assert!(w >= plain_w + 29.0 && w <= plain_w + 31.0, "the box's width is in the line: {w} against {plain_w}");
    assert_eq!(h, plain_h, "a box shorter than the text leaves the line as it was");
}

/// A box taller than the text, on the baseline, stands on it: the line grows
/// upwards by the difference and keeps the text's descent below.
#[test]
fn a_tall_box_on_the_baseline_grows_the_line_above_it() {
    let mut te = TextEngine::new();
    let text = "ab";
    let (_, plain_h) = te.measure(text, &base(), None);
    let rich = RichText { runs: vec![], items: vec![item(1, 10.0, 40.0, VerticalAlign::Baseline)] };
    let style = TextStyle { rich: Some(&rich), ..base() };
    let (_, h) = te.measure(text, &style, None);
    assert!(h > 40.0 && h < 40.0 + plain_h, "the box plus the descent: {h}");
    // Its bottom is on the baseline: the box ends above the line's bottom by
    // the text's descent, which is less than the plain line's height.
    let at = te.inline_boxes(text, &style, Align::Start, None);
    let bottom = at[0].1 + 40.0;
    assert!(at[0].1.abs() < 0.01, "the tallest thing starts the line: {}", at[0].1);
    assert!(bottom < h && h - bottom < plain_h / 2.0, "bottom {bottom} in a {h} line");
}

/// `middle` centres a box on the text's x-height: a box as tall as the text
/// sits lower than it does on the baseline.
#[test]
fn middle_sits_lower_than_baseline() {
    let mut te = TextEngine::new();
    let text = "ab";
    let at = |te: &mut TextEngine, align| {
        let rich = RichText { runs: vec![], items: vec![item(1, 10.0, 16.0, align)] };
        let style = TextStyle { rich: Some(&rich), ..base() };
        te.inline_boxes(text, &style, Align::Start, None)[0]
    };
    let on_baseline = at(&mut te, VerticalAlign::Baseline);
    let middle = at(&mut te, VerticalAlign::Middle);
    let top = at(&mut te, VerticalAlign::TextTop);
    let bottom = at(&mut te, VerticalAlign::TextBottom);
    assert!(middle.1 > on_baseline.1, "middle {} under baseline {}", middle.1, on_baseline.1);
    assert!(top.1.abs() < 0.01, "text-top starts at the top of the text: {}", top.1);
    assert!(bottom.1 > top.1, "text-bottom {} under text-top {}", bottom.1, top.1);
    // Left to right in the line all four were at the same x.
    assert_eq!(middle.0, on_baseline.0);
}

/// A box moves to the next line with the words around it.
#[test]
fn a_box_wraps_with_its_words() {
    let mut te = TextEngine::new();
    let text = "one two three four five six";
    let at_end = text.len() - 3;
    let rich = RichText { runs: vec![], items: vec![item(at_end, 12.0, 12.0, VerticalAlign::Middle)] };
    let style = TextStyle { rich: Some(&rich), ..base() };
    let (one_line_w, one_line_h) = te.measure(text, &style, None);
    let narrow = one_line_w / 2.0;
    let (_, h) = te.measure(text, &style, Some(narrow));
    assert!(h > one_line_h, "wrapped");
    let (x, y) = te.inline_boxes(text, &style, Align::Start, Some(narrow))[0];
    assert!(y >= one_line_h * 0.9, "on a later line: y {y}");
    assert!(x < narrow, "inside the box: x {x}");
}
