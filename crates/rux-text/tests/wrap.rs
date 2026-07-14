use rux_text::{TextEngine, Wrap};

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
        let (w, h) = te.measure(text, size, 400, Wrap::Normal, None);
        assert_eq!(w, w.trunc(), "{text}: width {w} is not whole-pixel");

        // Lay the text out again in exactly the box it asked for.
        let (_, h_in_box) = te.measure(text, size, 400, Wrap::Normal, Some(w));
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

    let (natural, _) = te.measure(long, 16.0, 400, Wrap::Normal, None);
    assert!(natural > box_width, "test needs a word wider than the box");

    // Normal: nothing can break it, so it overflows the box (as CSS does).
    let (w, h) = te.measure(long, 16.0, 400, Wrap::Normal, Some(box_width));
    assert!(w > box_width, "a lone long word has nowhere to break");
    let one_line = h;

    // break-word: it breaks inside the word and stays inside the box.
    let (w, h) = te.measure(long, 16.0, 400, Wrap::BreakWord, Some(box_width));
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

    let (x0, _, h) = te.caret_geometry(text, size, weight, wrap, None, 0);
    let (x5, _, _) = te.caret_geometry(text, size, weight, wrap, None, 5);
    let (xend, _, _) = te.caret_geometry(text, size, weight, wrap, None, text.len());
    assert_eq!(x0, 0.0, "caret at the start sits at the left edge");
    assert!(x5 > x0 && xend > x5, "the caret advances through the text");
    assert!(h > 0.0, "the caret has the line's height");

    // Clicking where the caret would be for index 5 comes back as index 5.
    let hit = te.index_at_point(text, size, weight, wrap, None, x5 + 0.5, h / 2.0);
    assert_eq!(hit, 5);

    // Clicking past the end lands at the end, not out of bounds.
    let hit = te.index_at_point(text, size, weight, wrap, None, xend + 500.0, h / 2.0);
    assert_eq!(hit, text.len());
}
