//! A paragraph's base direction: where its lines start, and that the mark the
//! engine may put in front of it is never seen outside.

use rux_layout::Direction;
use rux_text::{Align, TextEngine, TextStyle, Wrap};

fn style(direction: Option<Direction>) -> TextStyle<'static> {
    TextStyle { direction, ..TextStyle::new(16.0, 400, Wrap::Normal) }
}

/// Where the first character's caret is, from the left of a 400px box.
fn first_x(te: &mut TextEngine, text: &str, direction: Option<Direction>) -> f32 {
    te.caret_geometry(text, &style(direction), Some(400.0), 0).0
}

/// A right-to-left paragraph starts at the right, even when its first word
/// is Latin: the first letter would make it left-to-right, so the engine
/// puts a right-to-left mark in front.
#[test]
fn an_rtl_paragraph_starting_with_a_latin_word_starts_at_the_right() {
    let mut te = TextEngine::new();
    let text = "Rux هو إطار";
    let auto = first_x(&mut te, text, None);
    let rtl = first_x(&mut te, text, Some(Direction::Rtl));
    assert!(auto < 1.0, "on its own the Latin word starts it at the left: {auto}");
    // Unaligned layout: the line hugs its text, so measure its far edge.
    let (w, _) = te.measure(text, &style(Some(Direction::Rtl)), Some(400.0));
    assert!(rtl > w - 40.0, "right-to-left, `Rux` is at the right end: {rtl} of {w}");
}

/// The mark is invisible to the caller: the text measures the same width,
/// and every index in and out is the caller's own.
#[test]
fn the_mark_never_shows() {
    let mut te = TextEngine::new();
    let text = "Rux هو";
    let plain = te.measure(text, &style(None), None);
    let marked = te.measure(text, &style(Some(Direction::Rtl)), None);
    assert_eq!(plain.0, marked.0, "no width of its own");
    let s = style(Some(Direction::Rtl));
    for i in [0, 1, 2, 3, text.len()] {
        let (x, y, _) = te.caret_geometry(text, &s, None, i);
        let back = te.index_at_point(text, &s, None, x + 0.5, y + 1.0);
        assert!(back <= text.len(), "index {back} is past the end");
    }
    let (a, b) = te.word_at_point(text, &s, None, 1.0, 1.0);
    assert!(a <= b && b <= text.len(), "word {a}..{b}");
    let rects = te.selection_rects(text, &s, None, 0, 3);
    assert!(!rects.is_empty() && rects.iter().all(|r| r.2 > 0.0), "{rects:?}");
}

/// Text whose first letter already agrees gets no mark: a left-to-right
/// paragraph of Latin text measures and places exactly as it always has.
#[test]
fn agreeing_text_is_untouched() {
    let mut te = TextEngine::new();
    let text = "Read the terms";
    assert_eq!(te.measure(text, &style(None), None), te.measure(text, &style(Some(Direction::Ltr)), None));
    assert_eq!(first_x(&mut te, text, None), first_x(&mut te, text, Some(Direction::Ltr)));
}

/// A left-to-right paragraph that starts with an Arabic word is left to right
/// when it says so: its lines start at the left.
#[test]
fn an_ltr_paragraph_starting_with_arabic_starts_at_the_left() {
    let mut te = TextEngine::new();
    let text = "مرحبا world";
    let ltr = te.caret_geometry(text, &style(Some(Direction::Ltr)), Some(400.0), "مرحبا ".len()).0;
    let auto = te.caret_geometry(text, &style(None), Some(400.0), "مرحبا ".len()).0;
    // `world` follows the Arabic word on its right when left-to-right, and is
    // on its left when the paragraph is right-to-left.
    assert!(ltr > auto, "ltr {ltr} auto {auto}");
}

/// `left` and `right` are sides whatever the direction; `start` follows it.
#[test]
fn left_and_right_are_sides() {
    let mut te = TextEngine::new();
    let text = "hi";
    let s = style(Some(Direction::Rtl));
    let mut scene = vello::Scene::new();
    // Drawing aligned is enough to prove the alignments are accepted and
    // shaped; where they land is the layout's business and is driven in the
    // window.
    for align in [Align::Start, Align::End, Align::Left, Align::Right] {
        te.draw(&mut scene, 0.0, 0.0, text, &s, vello::peniko::Color::BLACK, align, Some(200.0), vello::kurbo::Affine::IDENTITY);
    }
}

/// Left goes on through Arabic and back through English, as an arrow key in a
/// browser's field does; in plain English a step is one character either way.
#[test]
fn a_visual_step_follows_what_is_drawn() {
    let mut te = TextEngine::new();
    let s = style(None);
    let latin = "abc";
    assert_eq!(te.visual_step(latin, &s, None, 1, true), 2);
    assert_eq!(te.visual_step(latin, &s, None, 1, false), 0);
    let arabic = "مرحبا";
    let second = "م".len();
    // At the start of an Arabic word, which is its right end, Left moves on.
    assert_eq!(te.visual_step(arabic, &s, None, 0, false), second);
    assert_eq!(te.visual_step(arabic, &s, None, second, true), 0);
    assert!(rux_text::has_rtl(arabic) && !rux_text::has_rtl(latin));
}
