//! `overflow` on a `<text>` bounds its own glyphs.
//!
//! A text has no children: its content is the glyphs. The clip used to be
//! pushed only around a node's children, so `height` plus `overflow: clip` on
//! a `<text>` still painted every line, down over whatever sat below it. Found
//! on the phone, in an event log written with exactly that CSS.

use rux_layout::Paint;
use rux_runtime::Document;

fn paints(style: &str) -> Vec<Paint> {
    let doc = Document::from_source(&format!(
        "<template><screen><text class=\"log\">{}</text></screen></template>\n\
         <style>.log {{ height: 20px; {style} }}</style>",
        "many words ".repeat(40)
    ))
    .expect("loads");
    rux_layout::layout(&doc.root, 200.0, 800.0, &mut |_, _| (10.0, 10.0)).paints
}

/// Where the glyphs sit, and whether a clip is open around them.
fn clipped_text(paints: &[Paint]) -> bool {
    let mut open = 0;
    for p in paints {
        match p {
            Paint::PushClip { height, .. } if *height == 20.0 => open += 1,
            Paint::PopClip if open > 0 => open -= 1,
            Paint::Text(_) => return open > 0,
            _ => {}
        }
    }
    panic!("no text was painted")
}

#[test]
fn a_clipped_text_clips_its_own_lines() {
    assert!(clipped_text(&paints("overflow: clip;")));
    assert!(clipped_text(&paints("overflow: hidden;")));
}

/// CSS's default is `visible`: the lines run out of the box, and nothing is
/// cut.
#[test]
fn a_text_left_visible_is_not_clipped() {
    assert!(!clipped_text(&paints("")));
}
