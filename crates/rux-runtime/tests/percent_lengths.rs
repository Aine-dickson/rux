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

/// On an oblong it resolves against the **shorter** side, which is a pill.
///
/// CSS would draw an ellipse here. Rux has one scalar per corner and cannot
/// represent that, and half the shorter side is the pill somebody writing `50%`
/// on a button is after. The divergence is deliberate and documented.
#[test]
fn a_percentage_radius_on_an_oblong_is_a_pill() {
    let d = doc("width: 160px; height: 60px; background: #fff; border-radius: 50%;");
    let found = radii(&d, 400.0, 400.0);
    assert!(found.contains(&[30.0; 4]), "half of the shorter side, 60: {found:?}");
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

/// The other three still have nothing to resolve against, and still say so.
#[test]
fn padding_margin_and_gap_still_report_a_percentage() {
    let d = doc("padding: 10%; margin: 5%; gap: 2%;");
    let said = format!("{:?}", d.diagnostics());
    for name in ["padding", "margin", "gap"] {
        assert!(
            said.contains(&format!("does not honor a percentage on `{name}` yet")),
            "`{name}` still reported: {said}"
        );
    }
    assert!(
        !said.contains("border-radius"),
        "and the radius is not dragged back in: {said}"
    );
}
