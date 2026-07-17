use rux_text::{TextEngine, TextStyle, Wrap};

fn style() -> TextStyle<'static> {
    TextStyle::new(16.0, 400, Wrap::Normal)
}

/// A selection covers the glyphs it names and nothing else: selecting a prefix
/// starts at the text's left edge and stops short of the full width.
#[test]
fn selection_covers_the_selected_run() {
    let mut te = TextEngine::new();
    let text = "hello world";
    let (full_w, _) = te.measure(text, &style(), None);

    let rects = te.selection_rects(text, &style(), None, 0, 5); // "hello"
    assert_eq!(rects.len(), 1, "one line, one rect");
    let (x, _, w, _) = rects[0];
    assert_eq!(x, 0.0, "a selection from index 0 starts at the left edge");
    assert!(w > 0.0 && w < full_w, "'hello' ({w}px) is a proper prefix of {full_w}px");
}

/// The empty selection is the collapsed one: a caret is not a highlight.
#[test]
fn a_collapsed_range_has_no_rects() {
    let mut te = TextEngine::new();
    assert!(te.selection_rects("hello", &style(), None, 3, 3).is_empty());
}

/// Selecting backwards is the same range as selecting forwards.
#[test]
fn the_range_is_normalized() {
    let mut te = TextEngine::new();
    let forward = te.selection_rects("hello world", &style(), None, 2, 7);
    let backward = te.selection_rects("hello world", &style(), None, 7, 2);
    assert_eq!(forward, backward);
}

/// **The alignment invariant.** We draw lines with the leading trimmed, stepping
/// by `ascent + descent` (or `line-height`), which is *not* parley's line pitch.
/// So the highlight's `y` must come from our stepping, or it drifts further from
/// the glyphs with every wrapped line.
#[test]
fn rects_line_up_with_our_own_line_stepping() {
    let mut te = TextEngine::new();
    let text = "aaaa bbbb cccc dddd eeee ffff";
    // Narrow enough to wrap into several lines.
    let max = Some(60.0);
    let (_, total_h) = te.measure(text, &style(), max);

    // Select everything: one rect per line, stacked with no gaps or overlaps,
    // and together exactly as tall as `measure` said the block is.
    let mut rects = te.selection_rects(text, &style(), max, 0, text.len());
    assert!(rects.len() > 1, "the text should have wrapped");
    rects.sort_by(|a, b| a.1.partial_cmp(&b.1).expect("finite"));

    assert_eq!(rects[0].1, 0.0, "the first line starts at the top of the box");
    for pair in rects.windows(2) {
        let (_, y0, _, h0) = pair[0];
        let (_, y1, _, _) = pair[1];
        assert_eq!(y0 + h0, y1, "line boxes must stack exactly, no gap or overlap");
    }
    let (_, last_y, _, last_h) = *rects.last().expect("a rect");
    assert_eq!(
        (last_y + last_h).ceil(),
        total_h,
        "the selection must end where the measured text does"
    );
}

/// A line-height changes our stepping, so the rects must follow it — this is the
/// case that would break if we took parley's own vertical coords.
#[test]
fn rects_follow_line_height() {
    let mut te = TextEngine::new();
    let text = "aaaa bbbb cccc";
    let max = Some(40.0);
    let tall = TextStyle { line_height: Some(40.0), ..style() };

    let rects = te.selection_rects(text, &tall, max, 0, text.len());
    assert!(rects.len() > 1, "the text should have wrapped");
    for (_, _, _, h) in &rects {
        assert_eq!(*h, 40.0, "each line box is the line-height tall");
    }
    let mut ys: Vec<f32> = rects.iter().map(|r| r.1).collect();
    ys.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
    assert_eq!(ys[1] - ys[0], 40.0, "lines are a line-height apart");
}

/// Double-click selects a word, not a character and not the whole line.
#[test]
fn word_at_point_picks_out_one_word() {
    let mut te = TextEngine::new();
    let text = "hello world";
    // Probe the middle of the last word. (Not `measure("hello ") + a nudge`:
    // parley trims trailing whitespace from a layout's width, so that lands on
    // the space, not the word — which is exactly what a caret-precision test
    // should be suspicious of.)
    let (full_w, h) = te.measure(text, &style(), None);
    let (word_w, _) = te.measure("world", &style(), None);
    let (start, end) = te.word_at_point(text, &style(), None, full_w - word_w / 2.0, h / 2.0);
    assert_eq!(&text[start..end], "world");

    // …and the first word, for good measure.
    let (start, end) = te.word_at_point(text, &style(), None, 2.0, h / 2.0);
    assert_eq!(&text[start..end], "hello");
}
