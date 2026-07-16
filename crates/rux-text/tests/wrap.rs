use rux_text::{TextEngine, TextStyle, Wrap};

/// Paint re-wraps text at the box width layout gave it, so the box must never
/// be narrower than the text measured. It used to come back fractional
/// (98.001px), get rounded down to a 98px box, and break the last word onto a
/// second line the box had no height for — the text then spilled over whatever
/// sat below it. Whether a string wrapped depended on which way its natural
/// width happened to round, which is why the glitch looked so arbitrary.
#[test]
fn measured_width_never_re_wraps_the_text() {
    let mut te = TextEngine::new();
    for (text, size) in [
        ("~ 8.2h remaining", 13.0f32),
        ("Sign in", 22.0),
        ("Your name", 16.0),
        ("Greetings from jllo.", 14.0),
        ("Greetings from jllojk.", 14.0),
    ] {
        let (w, h) = te.measure(text, &TextStyle::new(size, 400, Wrap::Normal), None);
        assert_eq!(w, w.trunc(), "{text}: width {w} is not whole-pixel");

        // Lay the text out again in exactly the box it asked for.
        let (_, h_in_box) = te.measure(text, &TextStyle::new(size, 400, Wrap::Normal), Some(w));
        assert_eq!(
            h_in_box, h,
            "{text}: wrapped to {h_in_box}px inside its own {w}px box (wanted {h}px)"
        );
    }
}

/// The reason we moved to parley 0.11: it can break *inside* a word. A word
/// wider than its box used to overflow with no way to stop it (parley 0.2 had
/// no overflow-wrap at all).
#[test]
fn break_word_breaks_inside_a_long_word() {
    let mut te = TextEngine::new();
    let long = "eknfenfenenelenlenlrenfennrelneflnlenflrenfelflnlefelnfelfnlnflenfelnfenlenlenelnlernlene";
    let box_width = 200.0;

    let (natural, _) = te.measure(long, &TextStyle::new(16.0, 400, Wrap::Normal), None);
    assert!(natural > box_width, "test needs a word wider than the box");

    // Normal: nothing can break it, so it overflows the box (as CSS does).
    let (w, h) = te.measure(long, &TextStyle::new(16.0, 400, Wrap::Normal), Some(box_width));
    assert!(w > box_width, "a lone long word has nowhere to break");
    let one_line = h;

    // break-word: it breaks inside the word and stays inside the box.
    let (w, h) = te.measure(long, &TextStyle::new(16.0, 400, Wrap::BreakWord), Some(box_width));
    assert!(w <= box_width, "break-word should fit the box, got {w}");
    assert!(h > one_line, "break-word should take multiple lines");
}

/// The caret: a byte index maps to an x position, and a click maps back to the
/// nearest index. Inputs used to only append/backspace at the end because we had
/// neither direction.
#[test]
fn caret_maps_between_index_and_point() {
    let mut te = TextEngine::new();
    let text = "hello world";
    let (size, weight, wrap) = (16.0f32, 400u16, Wrap::Normal);
    let style = TextStyle::new(size, weight, wrap);

    let (x0, _, h) = te.caret_geometry(text, &style, None, 0);
    let (x5, _, _) = te.caret_geometry(text, &style, None, 5);
    let (xend, _, _) = te.caret_geometry(text, &style, None, text.len());
    assert_eq!(x0, 0.0, "caret at the start sits at the left edge");
    assert!(x5 > x0 && xend > x5, "the caret advances through the text");
    assert!(h > 0.0, "the caret has the line's height");

    // Clicking where the caret would be for index 5 comes back as index 5.
    let hit = te.index_at_point(text, &style, None, x5 + 0.5, h / 2.0);
    assert_eq!(hit, 5);

    // Clicking past the end lands at the end, not out of bounds.
    let hit = te.index_at_point(text, &style, None, xend + 500.0, h / 2.0);
    assert_eq!(hit, text.len());
}

/// `font-family` actually reaches parley and changes shaping — not just that the
/// argument compiles. A very narrow proportional string ("illili") is far wider
/// in a monospace face, so the measured widths must differ. Generic families
/// (`monospace`) resolve on every platform via fontique, so this is portable.
#[test]
fn font_family_changes_shaping() {
    let mut te = TextEngine::new();
    let (text, size) = ("illililli", 40.0);
    let default = te.measure(text, &TextStyle::new(size, 400, Wrap::Normal), None).0;
    let mono = te.measure(text, &TextStyle { family: Some("monospace"), ..TextStyle::new(size, 400, Wrap::Normal) }, None).0;
    assert!(
        (default - mono).abs() > 1.0,
        "font-family had no effect: default={default}px, monospace={mono}px"
    );
    // An empty / whitespace family is ignored (falls back to the default face).
    let blank = te.measure(text, &TextStyle { family: Some("   "), ..TextStyle::new(size, 400, Wrap::Normal) }, None).0;
    assert_eq!(blank, default, "blank font-family should fall back to default");
}

/// `letter-spacing` adds space between glyphs, so a run gets measurably wider.
#[test]
fn letter_spacing_widens_the_run() {
    let mut te = TextEngine::new();
    let (text, size) = ("spacing", 24.0);
    let tight = te.measure(text, &TextStyle::new(size, 400, Wrap::Normal), None).0;
    let loose = te
        .measure(
            text,
            &TextStyle { letter_spacing: Some(6.0), ..TextStyle::new(size, 400, Wrap::Normal) },
            None,
        )
        .0;
    // Six letter gaps in "spacing" at +6px each ≈ +36px; allow slack.
    assert!(loose > tight + 20.0, "letter-spacing had no effect: {tight} → {loose}");
}

/// `line-height` sets each line box's height, so a taller line-height makes the
/// same wrapped text measure taller (and `None` keeps the leading-trimmed hug).
#[test]
fn line_height_grows_total_height() {
    let mut te = TextEngine::new();
    let text = "one two three four five six seven eight nine ten";
    let (size, box_w) = (20.0, 120.0);
    let (_, natural) = te.measure(text, &TextStyle::new(size, 400, Wrap::Normal), Some(box_w));
    let tall = TextStyle { line_height: Some(40.0), ..TextStyle::new(size, 400, Wrap::Normal) };
    let (_, tall_h) = te.measure(text, &tall, Some(box_w));
    assert!(tall_h > natural, "line-height:40 should exceed the natural height {natural}");
}

/// `white-space: nowrap` keeps everything on one line even in a narrow box,
/// where the same text otherwise wraps to several.
#[test]
fn nowrap_keeps_one_line() {
    let mut te = TextEngine::new();
    let text = "the quick brown fox jumps over the lazy dog";
    let (size, box_w) = (20.0, 120.0);

    let (_, wrapped_h) = te.measure(text, &TextStyle::new(size, 400, Wrap::Normal), Some(box_w));
    let (nowrap_w, nowrap_h) = te.measure(
        text,
        &TextStyle { nowrap: true, ..TextStyle::new(size, 400, Wrap::Normal) },
        Some(box_w),
    );
    assert!(wrapped_h > nowrap_h, "wrapped text should be taller than one line");
    assert!(nowrap_w > box_w, "nowrap text runs past the box width, as CSS does");
}
