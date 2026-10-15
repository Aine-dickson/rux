//! What is left out of a scene for being off screen: track (g).6.
//!
//! Wrong in one direction draws nothing where something should be, which is
//! the failure that matters; so each case here is a way for a paint to be
//! partly visible while its box, or its box before a transform, is not.

use std::cell::Cell;

use rux_layout::*;
use rux_paint::visible_paints;

const VIEW: (f32, f32) = (400.0, 300.0);

fn rect(x: f32, y: f32, w: f32, h: f32) -> Paint {
    Paint::Rect(PaintRect {
        x,
        y,
        width: w,
        height: h,
        background: Some(Background::Color(Rgba::new(1.0, 0.0, 0.0, 1.0))),
        radius: [0.0; 4],
        border: Sides::ZERO,
        border_color: None,
    })
}

fn content(s: &str, align: TextAlign) -> TextContent {
    TextContent {
        text: s.to_string(),
        font_size: 12.0,
        weight: 400,
        color: Rgba::new(1.0, 1.0, 1.0, 1.0),
        align,
        wrap: TextWrap::Normal,
        font_family: None,
        letter_spacing: None,
        word_spacing: None,
        line_height: None,
        italic: false,
        underline: false,
        strikethrough: false,
        nowrap: false,
        caret: None,
        selection: None,
        preedit: None,
        shadows: None,
        selection_style: Default::default(),
        rich: None,
        white_space: Default::default(),
        direction: None,
        isolate: Default::default(),
    }
}

fn text(x: f32, y: f32, w: f32, h: f32, align: TextAlign) -> Paint {
    Paint::Text(PaintText { x, y, width: w, height: h, content: content("some words", align) })
}

/// Visibility with a measurer that answers `size` and counts its calls.
fn shown(items: &[Paint], size: (f32, f32)) -> (Vec<bool>, usize) {
    let calls = Cell::new(0);
    let flags = visible_paints(items, VIEW, |_, _| {
        calls.set(calls.get() + 1);
        size
    });
    (flags, calls.get())
}

/// The case this is for: a long list, a window's worth on screen.
#[test]
fn a_long_list_keeps_only_the_rows_on_screen() {
    let items: Vec<Paint> = (0..100)
        .flat_map(|i| [rect(0.0, i as f32 * 20.0, 400.0, 20.0), text(8.0, i as f32 * 20.0 + 2.0, 300.0, 16.0, TextAlign::Start)])
        .collect();
    let (flags, calls) = shown(&items, (60.0, 16.0));
    let kept = flags.iter().filter(|f| **f).count();
    // 15 rows fill 300px; text slack lets the 16th's text in, never more.
    assert!((30..=33).contains(&kept), "kept {kept} of 200");
    assert!(flags[0] && flags[1] && flags[28] && flags[29], "the rows on screen");
    assert!(!flags[60] && !flags[199], "rows far below");
    assert_eq!(calls, 0, "text below the window is left out without measuring");
}

/// A box shorter than its lines lets them fall out of its bottom, into view.
#[test]
fn text_spilling_down_into_the_window_is_kept() {
    let items = [text(10.0, -100.0, 200.0, 20.0, TextAlign::Start)];
    assert_eq!(shown(&items, (180.0, 400.0)).0, [true], "measured 400 tall, it reaches down");
    assert_eq!(shown(&items, (180.0, 20.0)).0, [false], "measured as tall as its box, it does not");
}

/// Centred text wider than its box runs out of both sides.
#[test]
fn centred_overflow_reaching_back_into_the_window_is_kept() {
    let centred = [text(450.0, 10.0, 100.0, 20.0, TextAlign::Center)];
    assert_eq!(shown(&centred, (400.0, 20.0)).0, [true], "150px spills left of a box at 450");
    let start = [text(450.0, 10.0, 100.0, 20.0, TextAlign::Start)];
    assert_eq!(shown(&start, (400.0, 20.0)), (vec![false], 0), "start-aligned never runs left");
    // Running out of the right side of a box left of the window.
    let left = [text(-300.0, 10.0, 100.0, 20.0, TextAlign::Start)];
    assert_eq!(shown(&left, (400.0, 20.0)).0, [true]);
    assert_eq!(shown(&left, (100.0, 20.0)).0, [false]);
}

/// A box ending exactly where a clip begins still shades the pixel they
/// share when the edge is not on a pixel boundary (1.5x scale): seen as a
/// one-pixel line along a scroller's top when it was left out.
#[test]
fn a_box_flush_with_a_clip_edge_is_kept() {
    let items = [
        Paint::PushClip { x: 0.0, y: 100.0, width: 200.0, height: 100.0, radius: [0.0; 4] },
        rect(10.0, 50.0, 50.0, 50.0),
        rect(10.0, 40.0, 50.0, 50.0),
        Paint::PopClip,
    ];
    assert_eq!(shown(&items, (0.0, 0.0)).0, [true, true, false, true]);
}

/// A clip hides what is outside it, and only until it is popped.
#[test]
fn a_clip_hides_what_it_cuts_away_until_popped() {
    let items = [
        Paint::PushClip { x: 0.0, y: 0.0, width: 100.0, height: 100.0, radius: [8.0; 4] },
        rect(10.0, 10.0, 20.0, 20.0),
        rect(150.0, 10.0, 20.0, 20.0),
        Paint::PopClip,
        rect(150.0, 10.0, 20.0, 20.0),
    ];
    assert_eq!(shown(&items, (0.0, 0.0)).0, [true, true, false, true, true]);
}

/// A transform carries a box into the window, out of it, or flattens it.
#[test]
fn a_transform_moves_what_is_visible() {
    let up = [Paint::PushTransform([1.0, 0.0, 0.0, 1.0, 0.0, -500.0]), rect(10.0, 600.0, 20.0, 20.0), Paint::PopTransform, rect(10.0, 600.0, 20.0, 20.0)];
    assert_eq!(shown(&up, (0.0, 0.0)).0, [true, true, true, false], "moved up 500px into view");
    // A quarter turn about the origin takes (100, -50) to (50, 100).
    let turned = [Paint::PushTransform([0.0, 1.0, -1.0, 0.0, 0.0, 0.0]), rect(100.0, -60.0, 20.0, 20.0), Paint::PopTransform];
    assert_eq!(shown(&turned, (0.0, 0.0)).0, [true, true, true], "rotated into view");
    let flat = [Paint::PushTransform([0.0; 6]), rect(10.0, 10.0, 20.0, 20.0), Paint::PopTransform];
    assert_eq!(shown(&flat, (0.0, 0.0)).0, [true, false, true], "scaled to nothing draws nothing");
    // Inside a clip, inside a transform: the clip is in the transform's terms.
    let both = [
        Paint::PushTransform([2.0, 0.0, 0.0, 2.0, 0.0, 0.0]),
        Paint::PushClip { x: 0.0, y: 0.0, width: 50.0, height: 50.0, radius: [0.0; 4] },
        rect(40.0, 40.0, 5.0, 5.0),
        rect(60.0, 10.0, 5.0, 5.0),
        Paint::PopClip,
        Paint::PopTransform,
    ];
    assert_eq!(shown(&both, (0.0, 0.0)).0, [true, true, true, false, true, true]);
}

/// A shadow's blur and a stroke's corners reach past their boxes.
#[test]
fn blur_and_stroke_reach_past_their_boxes() {
    let shadow = |y: f32| Paint::Shadow { x: 0.0, y, width: 100.0, height: 20.0, radius: 0.0, blur: 30.0, color: Rgba::new(0.0, 0.0, 0.0, 0.5) };
    assert_eq!(shown(&[shadow(-70.0)], (0.0, 0.0)).0, [true], "a 30px blur reaches 50px down");
    assert_eq!(shown(&[shadow(-200.0)], (0.0, 0.0)).0, [false]);
    let path = |stroke: f32| {
        Paint::Path(PaintPath {
            x: 0.0,
            y: -30.0,
            content: PathContent::parse("M 0 0 L 100 10 L 0 20"),
            paint: PathPaint {
                fill: None,
                fill_rule: FillRule::NonZero,
                stroke: Some(Rgba::new(1.0, 1.0, 1.0, 1.0)),
                stroke_width: stroke,
                cap: LineCap::Butt,
                join: LineJoin::Miter,
            },
        })
    };
    assert_eq!(shown(&[path(4.0)], (0.0, 0.0)).0, [true], "a 4px stroke's miter can reach 10px down");
    assert_eq!(shown(&[path(1.0)], (0.0, 0.0)).0, [false]);
}
