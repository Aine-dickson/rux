//! What a text field's editing keys ask of the text: a word at a time, the
//! ends of a wrapped line, and the line or paragraph a triple click takes.

use rux_layout::Direction;
use rux_text::{TextEngine, TextStyle, Wrap};

fn style() -> TextStyle<'static> {
    TextStyle::new(16.0, 400, Wrap::Normal)
}

/// Ctrl+Right stops at each word, Ctrl+Left goes back the same way.
#[test]
fn a_word_at_a_time() {
    let mut te = TextEngine::new();
    let text = "one two three";
    let s = style();
    let mut stops = vec![0];
    while *stops.last().unwrap() < text.len() {
        let at = *stops.last().unwrap();
        let next = te.word_step(text, &s, None, at, true);
        assert!(next > at, "stuck at {at}");
        stops.push(next);
    }
    // Each stop is at a word's edge: never inside one.
    for &i in &stops {
        let inside = i > 0 && i < text.len() && text.as_bytes()[i - 1] != b' ' && text.as_bytes()[i] != b' ';
        assert!(!inside, "stopped inside a word at {i}: {stops:?}");
    }
    assert!(stops.len() >= 4, "three words, so at least three steps: {stops:?}");
    let back = te.word_step(text, &s, None, text.len(), false);
    assert_eq!(back, 8, "Ctrl+Left from the end is the start of `three`");
}

/// In Arabic, Ctrl+Left goes on into the text, which is drawn leftwards.
#[test]
fn a_word_at_a_time_in_right_to_left_text() {
    let mut te = TextEngine::new();
    let text = "مرحبا بالعالم";
    let s = TextStyle { direction: Some(Direction::Rtl), ..style() };
    let to = te.word_step(text, &s, None, 0, false);
    assert!(to > 0, "Left from the start of an Arabic line moves on into it: {to}");
}

/// Ctrl+Backspace deletes back to the start of the word, Ctrl+Delete on to
/// its end, in the text's own order.
#[test]
fn word_edges_for_deleting() {
    let mut te = TextEngine::new();
    let text = "one two three";
    let s = style();
    assert_eq!(te.word_edge(text, &s, None, 7, false), 4, "back from the end of `two`");
    let end = te.word_edge(text, &s, None, 4, true);
    assert!(end == 7 || end == 8, "on from the start of `two` to its end: {end}");
}

/// Home and End in a wrapped field go to the ends of the line the caret is
/// on, not of the whole text.
#[test]
fn the_ends_of_a_wrapped_line() {
    let mut te = TextEngine::new();
    let text = "alpha beta gamma delta epsilon zeta eta theta iota kappa";
    let s = style();
    let width = Some(120.0);
    let (_, y0, _) = te.caret_geometry(text, &s, width, 0);
    // A caret on the second line.
    let i = (0..text.len())
        .filter(|i| text.is_char_boundary(*i))
        .find(|i| te.caret_geometry(text, &s, width, *i).1 > y0 + 1.0)
        .expect("the text wraps");
    let mid = i + 2;
    let start = te.line_edge(text, &s, width, mid, false);
    let end = te.line_edge(text, &s, width, mid, true);
    assert!(start > 0, "not the start of the text");
    assert!(start <= mid && end >= mid, "{start}..{end} holds {mid}");
    assert!(end < text.len(), "not the end of the text");
    assert_ne!(&text[end - 1..end], " ", "End stops before the space the line wraps at");
    let (_, y_mid, _) = te.caret_geometry(text, &s, width, mid);
    let (_, y_end, _) = te.caret_geometry(text, &s, width, end);
    assert_eq!(y_mid, y_end, "and the caret stays on the same line");
}

/// A triple click takes the wrapped line, or with `paragraph` everything up
/// to the line break.
#[test]
fn the_line_or_paragraph_under_a_point() {
    let mut te = TextEngine::new();
    let text = "first paragraph that wraps over lines\nsecond";
    let s = style();
    let width = Some(120.0);
    let (line_start, line_end) = te.line_at_point(text, &s, width, 5.0, 5.0, false);
    assert_eq!(line_start, 0);
    assert!(line_end < 37, "one wrapped line, not the paragraph: {line_end}");
    let (p_start, p_end) = te.line_at_point(text, &s, width, 5.0, 5.0, true);
    assert_eq!(p_start, 0);
    assert!(p_end >= 37 && p_end <= 38, "the whole first paragraph: {p_end}");
}
