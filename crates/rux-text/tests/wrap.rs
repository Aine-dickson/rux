use rux_text::TextEngine;

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
        let (w, h) = te.measure(text, size, 400, None);
        assert_eq!(w, w.trunc(), "{text}: width {w} is not whole-pixel");

        // Lay the text out again in exactly the box it asked for.
        let (_, h_in_box) = te.measure(text, size, 400, Some(w));
        assert_eq!(
            h_in_box, h,
            "{text}: wrapped to {h_in_box}px inside its own {w}px box (wanted {h}px)"
        );
    }
}
