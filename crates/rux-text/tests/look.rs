//! The rarely set text properties as the engine shapes them: `text-indent`
//! and `text-transform`.

use rux_layout::{TextLook, TextTransform};
use rux_text::{TextEngine, TextStyle, Wrap};

fn style(look: Option<&TextLook>) -> TextStyle<'_> {
    TextStyle { look, ..TextStyle::new(16.0, 400, Wrap::Normal) }
}

/// The first line starts `text-indent` further in, so a line that does not
/// wrap is that much wider; a percentage is of the width it is set in.
#[test]
fn an_indent_moves_the_first_line_in() {
    let mut te = TextEngine::new();
    let plain = te.measure("indented", &style(None), None).0;
    let look = TextLook { indent: 40.0, ..Default::default() };
    let indented = te.measure("indented", &style(Some(&look)), None).0;
    assert!((indented - plain - 40.0).abs() < 1.0, "{plain} then {indented}");
    let pct = TextLook { indent_pct: Some(10.0), ..Default::default() };
    let w = te.measure("indented", &style(Some(&pct)), Some(400.0)).0;
    assert!((w - plain - 40.0).abs() < 1.0, "10% of 400 is 40: {w}");
}

/// `hanging` indents every line but the first: one line is not moved.
#[test]
fn a_hanging_indent_leaves_one_line_alone() {
    let mut te = TextEngine::new();
    let plain = te.measure("one line", &style(None), None).0;
    let look = TextLook { indent: 40.0, indent_hanging: true, ..Default::default() };
    let hung = te.measure("one line", &style(Some(&look)), None).0;
    assert!((hung - plain).abs() < 1.0, "{plain} then {hung}");
}

/// Capitals are wider than small letters, so an uppercased word measures
/// wider: the transform reaches the shaping, not only the drawing.
#[test]
fn a_transform_is_shaped() {
    let mut te = TextEngine::new();
    let lower = te.measure("rux", &style(None), None).0;
    let look = TextLook { transform: TextTransform::Uppercase, ..Default::default() };
    let upper = te.measure("rux", &style(Some(&look)), None).0;
    assert!(upper > lower, "{lower} then {upper}");
    assert_eq!(upper, te.measure("RUX", &style(None), None).0);
}
