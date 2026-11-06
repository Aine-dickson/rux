//! Percentages on the properties that are resolved to plain pixels, and the one
//! that is not.
//!
//! `border-radius` is the exception: it is never consumed by layout, only by
//! paint, and paint knows the box. So a percentage there has somewhere to
//! resolve and is honored. `padding`, `margin` and `gap` are read during the
//! cascade, where there is genuinely no box, and are still reported.
//!
//! Reported twice by the same person, the second time with a screenshot of a
//! square button carrying `border-radius: 100%`.

use rux_layout::{layout, Paint};
use rux_runtime::Document;

/// Lay a document out and hand back every rounded rectangle it painted.
fn radii(doc: &Document, width: f32, height: f32) -> Vec<[f32; 4]> {
    let mut measure = |_: &rux_layout::TextContent, _: Option<f32>| (0.0, 0.0);
    let out = layout(&doc.root, width, height, &mut measure);
    out.paints
        .iter()
        .filter_map(|p| match p {
            Paint::Rect(r) => Some(r.radius),
            _ => None,
        })
        .collect()
}

fn doc(style: &str) -> Document {
    Document::from_source(&format!(
        "<template><screen><view class=\"a\"></view></screen></template>
         <style>
           .a {{ {style} }}
         </style>"
    ))
    .expect("loads")
}

/// `border-radius: 50%` on a square is half its side, which is a circle.
#[test]
fn a_percentage_radius_resolves_against_the_box() {
    let d = doc("width: 120px; height: 120px; background: #fff; border-radius: 50%;");
    let found = radii(&d, 400.0, 400.0);
    assert!(found.contains(&[60.0; 4]), "half of 120: {found:?}");
    assert!(d.diagnostics().warnings.is_empty(), "and says nothing: {:?}", d.diagnostics());
}

/// Both radii of every rounded rectangle painted: across, then down.
fn ellipses(doc: &Document) -> Vec<([f32; 4], Option<[f32; 4]>)> {
    let mut measure = |_: &rux_layout::TextContent, _: Option<f32>| (0.0, 0.0);
    let out = layout(&doc.root, 400.0, 400.0, &mut measure);
    out.paints
        .iter()
        .filter_map(|p| match p {
            Paint::Rect(r) => Some((r.radius, r.radius_y)),
            _ => None,
        })
        .collect()
}

/// On an oblong a percentage is of the width across and of the height down,
/// which is an ellipse, as in CSS (MS-013).
#[test]
fn a_percentage_radius_on_an_oblong_is_an_ellipse() {
    let d = doc("width: 160px; height: 60px; background: #fff; border-radius: 50%;");
    let found = ellipses(&d);
    assert!(found.contains(&([80.0; 4], Some([30.0; 4]))), "80 across, 30 down: {found:?}");
}

/// The part after the `/` is the vertical radius, in px or in a percentage.
#[test]
fn a_slash_gives_the_vertical_radius() {
    let d = doc("width: 100px; height: 80px; background: #fff; border-radius: 10px / 20px;");
    assert!(ellipses(&d).contains(&([10.0; 4], Some([20.0; 4]))));
    let d = doc("width: 100px; height: 80px; background: #fff; border-radius: 10px 20px / 25% 50%;");
    let want = ([10.0, 20.0, 10.0, 20.0], Some([20.0, 40.0, 20.0, 40.0]));
    assert!(ellipses(&d).contains(&want), "{:?}", ellipses(&d));
}

/// A longhand can take two values, across then down.
#[test]
fn a_longhand_takes_two_values() {
    let d = doc("width: 100px; height: 80px; background: #fff; border-top-left-radius: 30px 10px;");
    let found = ellipses(&d);
    assert!(found.contains(&([30.0, 0.0, 0.0, 0.0], Some([10.0, 0.0, 0.0, 0.0]))), "{found:?}");
}

/// A circle written both ways stays a circle: no vertical radius to carry.
#[test]
fn equal_radii_carry_no_vertical() {
    let d = doc("width: 100px; height: 80px; background: #fff; border-radius: 12px / 12px;");
    assert!(ellipses(&d).contains(&([12.0; 4], None)));
}

/// The diagonal grouping survives, so a percentage can round two corners only.
#[test]
fn percentage_corners_keep_the_diagonal_grouping() {
    let d = doc("width: 100px; height: 100px; background: #fff; border-radius: 50% 0px;");
    let found = radii(&d, 400.0, 400.0);
    assert!(found.contains(&[50.0, 0.0, 50.0, 0.0]), "TL and BR only: {found:?}");
}

/// A longhand in px after a shorthand in % replaces that corner in both units.
///
/// Without clearing the percentage the corner would go on resolving against the
/// box and the pixels would never be drawn, which is the silent-drop shape this
/// whole change exists to remove.
#[test]
fn a_px_longhand_beats_a_percentage_shorthand() {
    let d = doc(
        "width: 100px; height: 100px; background: #fff; \
         border-radius: 50%; border-top-left-radius: 4px;",
    );
    let found = radii(&d, 400.0, 400.0);
    assert!(found.contains(&[4.0, 50.0, 50.0, 50.0]), "TL in px, the rest in %: {found:?}");
}

/// Nothing changes for a document that never writes a percentage.
#[test]
fn a_pixel_radius_is_untouched() {
    let d = doc("width: 100px; height: 40px; background: #fff; border-radius: 8px;");
    let found = radii(&d, 400.0, 400.0);
    assert!(found.contains(&[8.0; 4]), "8px stays 8px: {found:?}");
}

/// Every coloured box a document painted, by its red channel, as
/// `(x, y, width, height)`.
fn boxes(src: &str, width: f32, height: f32) -> (Document, Vec<(f32, [f32; 4])>) {
    let d = Document::from_source(src).expect("loads");
    let mut measure = |_: &rux_layout::TextContent, _: Option<f32>| (0.0, 0.0);
    let out = layout(&d.root, width, height, &mut measure);
    let found = out
        .paints
        .iter()
        .filter_map(|p| match p {
            Paint::Rect(r) => match &r.background {
                Some(rux_layout::Background::Color(c)) => Some((c.r, [r.x, r.y, r.width, r.height])),
                _ => None,
            },
            _ => None,
        })
        .collect();
    (d, found)
}

fn at(found: &[(f32, [f32; 4])], red: f32) -> [f32; 4] {
    found.iter().find(|(r, _)| (*r - red).abs() < 0.01).map(|(_, b)| *b).expect("painted")
}

/// `padding`, `margin` and `gap` take a percentage now, resolved by the
/// layout as CSS resolves it: a side against the containing block's
/// **width** (the top and bottom too), a gap against the box's own content
/// size on its axis. Watchlist #6: they were reported and dropped.
#[test]
fn padding_margin_and_gap_resolve_a_percentage() {
    let (d, found) = boxes(
        "<template><screen class=\"s\"><view class=\"a\"><view class=\"b\"></view><view class=\"c\"></view></view></screen></template>
         <style>
           .s { padding: 0; }
           .a { display: flex; flex-direction: column; width: 200px; height: 200px;
                margin: 5%; padding: 10% 0 0 10%; gap: 10%; background: rgb(255, 0, 0); }
           .b { width: 20px; height: 20px; background: rgb(0, 0, 255); }
           .c { width: 20px; height: 20px; background: rgb(0, 255, 0); }
         </style>",
        400.0,
        400.0,
    );
    let said = format!("{:?}", d.diagnostics());
    assert!(!said.contains("percentage"), "nothing is reported: {said}");
    let a = at(&found, 1.0);
    let b = at(&found, 0.0);
    // 5% of the screen's 400 is 20, on the left and on the top alike.
    assert_eq!((a[0], a[1]), (20.0, 20.0), "the margin is 5% of the width, on every side");
    // 10% of the containing block's width (the screen's 400) is 40, top and left.
    assert_eq!((b[0] - a[0], b[1] - a[1]), (40.0, 40.0), "so is the padding");
    // The gap is 10% of the box's content height, which under CSS's
    // `content-box` is the 200 written, so 20 between the two children.
    // The two children, blue then green, both with no red in them.
    let kids = found.iter().filter(|(r, _)| *r == 0.0).map(|(_, x)| *x).collect::<Vec<_>>();
    let (first, second) = (kids[0], kids[1]);
    assert_eq!(second[1] - (first[1] + first[3]), 20.0, "and the gap is of the content height: {found:?}");
}

/// A percentage keeps its place in the shorthand. It used to be dropped,
/// which moved the values after it: `padding: 10px 5%` was 10px all round.
#[test]
fn a_percentage_keeps_its_place_in_the_shorthand() {
    let (_, found) = boxes(
        "<template><screen class=\"s\"><view class=\"a\"><view class=\"b\"></view></view></screen></template>
         <style>
           .s { padding: 0; align-items: flex-start; }
           .a { padding: 10px 5%; background: rgb(255, 0, 0); }
           .b { width: 20px; height: 20px; background: rgb(0, 0, 255); }
         </style>",
        400.0,
        400.0,
    );
    let a = at(&found, 1.0);
    let b = at(&found, 0.0);
    assert_eq!((b[0] - a[0], b[1] - a[1]), (20.0, 10.0), "left 5% of 400, top 10px: {found:?}");
}
