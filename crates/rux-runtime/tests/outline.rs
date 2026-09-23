//! `outline`, and the focus ring that is now one.
//!
//! The ring used to be drawn by the shell, 2px of a fixed blue at a fixed
//! radius, over every focused element, and nothing an author wrote could touch
//! it. It is now the default stylesheet's `:focus-visible { outline: auto }`,
//! so it is restyled and removed the way CSS restyles and removes it, and
//! `outline` works on any element, focused or not.

use rux_layout::{layout, Outline, Paint, PaintRect, Rgba};
use rux_runtime::Document;
use rux_style::InteractionState;

fn paints(doc: &Document) -> Vec<Paint> {
    let mut measure = |_: &rux_layout::TextContent, _: Option<f32>| (40.0, 18.0);
    layout(&doc.root, 400.0, 600.0, &mut measure).paints
}

/// Every stroked rect with no fill: what an outline paints as.
fn outlines(doc: &Document) -> Vec<PaintRect> {
    paints(doc)
        .into_iter()
        .filter_map(|p| match p {
            Paint::Rect(r) if r.background.is_none() && r.border_color.is_some() => Some(r),
            _ => None,
        })
        .collect()
}

fn same(a: Rgba, b: Rgba) -> bool {
    (a.r - b.r).abs() < 0.01 && (a.g - b.g).abs() < 0.01 && (a.b - b.b).abs() < 0.01
}

fn doc(markup: &str, style: &str) -> Document {
    Document::from_source(&format!(
        "<template><screen>{markup}</screen></template>
         <style>{style}</style>
         <script>let a = signal(0); let q = signal(\"\");</script>"
    ))
    .expect("loads")
}

/// A plain outline, on an element nobody has focused: outside the box by its
/// width, in the colour written.
#[test]
fn an_outline_is_drawn_outside_the_box() {
    let d = doc(
        "<view class=\"a\"></view>",
        ".a { width: 100px; height: 50px; outline: 2px solid #ff0000; }",
    );
    let found = outlines(&d);
    assert_eq!(found.len(), 1, "{found:?}");
    let o = &found[0];
    assert_eq!((o.x, o.y, o.width, o.height), (-2.0, -2.0, 104.0, 54.0));
    assert_eq!(o.border.top, 2.0);
    assert!(same(o.border_color.unwrap(), Rgba::new(1.0, 0.0, 0.0, 1.0)));
    assert!(d.diagnostics().warnings.is_empty(), "honoured, so nothing to say: {:?}", d.diagnostics());
}

/// `outline-offset` moves it out, and a negative one in. The longhands
/// override the shorthand one part at a time.
#[test]
fn the_longhands_and_the_offset() {
    let d = doc(
        "<view class=\"a\"></view><view class=\"b\"></view>",
        ".a { width: 100px; height: 50px; outline: 2px solid #ff0000; outline-width: 4px; outline-offset: 3px; }
         .b { width: 100px; height: 50px; outline-style: solid; outline-width: 1px; outline-offset: -5px; }",
    );
    let found = outlines(&d);
    assert_eq!(found.len(), 2, "{found:?}");
    assert_eq!((found[0].x, found[0].width, found[0].border.top), (-7.0, 114.0, 4.0));
    assert_eq!((found[1].x, found[1].width, found[1].border.top), (-(-5.0 + 1.0), 92.0, 1.0));
}

/// With no colour written it is the element's `color`, CSS's `currentColor`.
#[test]
fn an_outline_with_no_colour_takes_the_text_colour() {
    let d = doc("<view class=\"a\"></view>", ".a { width: 10px; height: 10px; color: #00ff00; outline: solid; }");
    let found = outlines(&d);
    assert_eq!(found.len(), 1);
    assert!(same(found[0].border_color.unwrap(), Rgba::new(0.0, 1.0, 0.0, 1.0)), "{found:?}");
    assert_eq!(found[0].border.top, 3.0, "and CSS's initial width, medium");
}

/// `none`, `hidden` and a zero width all mean no outline, `outline: 0` above
/// all, which is how stylesheets have removed one for twenty years.
#[test]
fn none_and_zero_draw_nothing() {
    for rule in ["outline: none", "outline: 0", "outline: 2px hidden red", "outline-width: 2px"] {
        let d = doc("<view class=\"a\"></view>", &format!(".a {{ width: 10px; height: 10px; {rule}; }}"));
        assert!(outlines(&d).is_empty(), "{rule}: {:?}", outlines(&d));
    }
}

/// It follows a rounded corner, grown by how far out it is; a square corner
/// stays square.
#[test]
fn an_outline_follows_the_radius() {
    let d = doc(
        "<view class=\"a\"></view>",
        ".a { width: 100px; height: 50px; border-radius: 8px 0px; outline: 2px solid red; outline-offset: 1px; }",
    );
    let found = outlines(&d);
    assert_eq!(found[0].radius, [11.0, 0.0, 11.0, 0.0], "{found:?}");
}

/// Drawn over what the element contains, and outside the element's own clip:
/// `overflow: hidden` on the element does not cut its own outline.
#[test]
fn an_outline_is_over_the_content_and_outside_its_own_clip() {
    let d = doc(
        "<view class=\"a\"><view class=\"kid\"></view></view>",
        ".a { width: 100px; height: 50px; overflow: hidden; outline: 2px solid red; }
         .kid { width: 20px; height: 20px; background: #0000ff; }",
    );
    let all = paints(&d);
    let kid = all
        .iter()
        .position(|p| matches!(p, Paint::Rect(r) if r.background.is_some()))
        .expect("the child's box");
    let pop = all.iter().position(|p| matches!(p, Paint::PopClip)).expect("the element's clip ends");
    let ring = all
        .iter()
        .position(|p| matches!(p, Paint::Rect(r) if r.background.is_none() && r.border_color.is_some()))
        .expect("the outline");
    assert!(kid < pop && pop < ring, "child {kid}, clip popped {pop}, outline {ring}");
}

/// Inside a scroller it is inside the scroller's clip, like everything else
/// the scroller holds. The shell's ring had to be taught this separately, and
/// once drew over a paragraph above a list it had scrolled out of.
#[test]
fn an_outline_in_a_scroller_is_clipped_by_it() {
    let d = doc(
        "<view class=\"list\"><view class=\"row\"></view></view>",
        ".list { height: 60px; overflow-y: auto; }
         .row { height: 200px; outline: 2px solid red; }",
    );
    let all = paints(&d);
    let push = all.iter().position(|p| matches!(p, Paint::PushClip { .. })).expect("the list clips");
    let ring = all
        .iter()
        .position(|p| matches!(p, Paint::Rect(r) if r.background.is_none() && r.border_color.is_some()))
        .expect("the outline");
    let pop = all.iter().rposition(|p| matches!(p, Paint::PopClip)).expect("and stops");
    assert!(push < ring && ring < pop, "clip {push}, outline {ring}, pop {pop}");
}

/// A dashed or dotted outline is drawn solid, as a border is, and says so.
#[test]
fn a_style_rux_cannot_draw_is_drawn_solid_and_said() {
    let d = doc("<view class=\"a\"></view>", ".a { width: 10px; height: 10px; outline: 2px dashed red; }");
    assert_eq!(outlines(&d).len(), 1);
    let said = format!("{:?}", d.diagnostics());
    assert!(said.contains("drawn solid"), "{said}");
}

// ---- The focus ring ---------------------------------------------------------

/// Focus the `@tap` box at `path`, the way the shell does after a Tab (`true`)
/// or a tap (`false`).
fn focus(d: &mut Document, path: &[usize], keyboard: bool) {
    d.set_interaction(InteractionState {
        focused_path: Some(path.to_vec()),
        focus_visible: keyboard,
        ..InteractionState::default()
    });
}

fn button(style: &str) -> Document {
    doc("<view class=\"b\" @tap=\"a += 1\"></view>", &format!(".b {{ width: 80px; height: 40px; }} {style}"))
}

/// Reached with Tab, a button shows the ring: the platform's colour, 2px,
/// just outside the box.
#[test]
fn a_button_focused_from_the_keyboard_shows_the_ring() {
    let mut d = button("");
    assert!(outlines(&d).is_empty(), "nothing is focused yet");
    focus(&mut d, &[0], true);
    let found = outlines(&d);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(same(found[0].border_color.unwrap(), Outline::RING));
    assert_eq!((found[0].x, found[0].border.top), (-2.0, 2.0));
    assert_eq!(found[0].radius, [4.0; 4], "a ring is a little round even on a square box");
}

/// Tapped, the same button is focused but shows nothing, which is what a
/// browser does and what a phone does.
#[test]
fn a_button_focused_by_a_tap_shows_no_ring() {
    let mut d = button(".b:focus { background: #222222; }");
    focus(&mut d, &[0], false);
    assert!(outlines(&d).is_empty(), "{:?}", outlines(&d));
    let filled = paints(&d)
        .iter()
        .any(|p| matches!(p, Paint::Rect(r) if r.background.is_some()));
    assert!(filled, "but it is focused: `:focus` still matches it");
}

/// `outline: none` takes the ring away.
#[test]
fn outline_none_removes_the_ring() {
    let mut d = button(".b:focus-visible { outline: none; }");
    focus(&mut d, &[0], true);
    assert!(outlines(&d).is_empty(), "{:?}", outlines(&d));
}

/// An author's outline replaces the ring rather than joining it.
#[test]
fn an_authors_outline_replaces_the_ring() {
    let mut d = button(".b:focus-visible { outline: 3px solid #ff0000; outline-offset: 2px; }");
    focus(&mut d, &[0], true);
    let found = outlines(&d);
    assert_eq!(found.len(), 1, "one line, the author's: {found:?}");
    assert!(same(found[0].border_color.unwrap(), Rgba::new(1.0, 0.0, 0.0, 1.0)));
    assert_eq!(found[0].border.top, 3.0);
}

/// Only the colour written: the ring keeps its shape and takes the colour.
#[test]
fn the_ring_can_be_recoloured_alone() {
    let mut d = button(".b { outline-color: #ff0000; }");
    focus(&mut d, &[0], true);
    let found = outlines(&d);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(same(found[0].border_color.unwrap(), Rgba::new(1.0, 0.0, 0.0, 1.0)));
    assert_eq!(found[0].border.top, 2.0, "still the ring's width");
}

/// `:focus` matches the element itself, not its ancestors, and not a button
/// next to it.
#[test]
fn focus_matches_only_the_focused_element() {
    let mut d = doc(
        "<view class=\"card\"><view class=\"b\" @tap=\"a = 1\"></view><view class=\"b\" @tap=\"a = 2\"></view></view>",
        ".b { width: 20px; height: 20px; } .card:focus, .b:focus { background: #ff0000; }",
    );
    focus(&mut d, &[0, 1], false);
    let red: Vec<PaintRect> = paints(&d)
        .into_iter()
        .filter_map(|p| match p {
            Paint::Rect(r) if r.background.is_some() => Some(r),
            _ => None,
        })
        .collect();
    assert_eq!(red.len(), 1, "the second button only: {red:?}");
    assert_eq!(red[0].x, 0.0);
    assert_eq!(red[0].y, 20.0);
}

/// A text field shows its focus however it got it: it is about to be typed
/// into, and the ring is where the typing will go.
#[test]
fn a_focused_text_field_shows_the_ring_after_a_tap() {
    let mut d = doc("<input class=\"f\" r-model=\"q\" />", ".f { width: 120px; height: 30px; }");
    d.set_interaction(InteractionState {
        focused_model: Some("q".to_string()),
        ..InteractionState::default()
    });
    let found = outlines(&d);
    assert!(
        found.iter().any(|r| same(r.border_color.unwrap(), Outline::RING)),
        "the ring, with no keyboard involved: {found:?}"
    );
}
