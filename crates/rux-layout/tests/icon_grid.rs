//! Fitting a fixed drawing grid into an arbitrary box, with no `transform-origin`.
//!
//! **This is the question the icon element is blocked on.** An icon set is
//! drawn on one grid, Tabler's being 24 by 24, and an author asks for it at
//! whatever size the surrounding type is. `width` on a path sets the box and
//! does not scale the drawing, and `transform-origin` is unimplemented: the
//! origin is fixed at the box centre. The obvious reading is that the grid
//! cannot be mapped onto the box.
//!
//! It can. Transform functions compose, so a translate cancels the fixed
//! centre. For a `GRID`-unit drawing in a box of `n` pixels:
//!
//! ```text
//! scale(n / GRID) translate(n / 2 - GRID / 2 px, ...)
//! ```
//!
//! These tests assert that against the real layout pass rather than against the
//! algebra that produced it, because the algebra is what is in doubt. If they
//! ever fail, the `size` attribute on `<icon>` is wrong and so is everything
//! built on it.

use rux_layout::*;

/// The grid an icon set is drawn on. Tabler, Lucide and Phosphor all use 24.
const GRID: f32 = 24.0;

/// The transform an icon of `n` pixels has to emit to fill its own box.
fn icon_transform(n: f32) -> Transform {
    let s = n / GRID;
    let t = n / 2.0 - GRID / 2.0;
    // scale(s) then translate(t, t), composed the way `transform: scale() translate()`
    // composes: leftmost outermost, so the translate happens first in point order.
    let scale = [s, 0.0, 0.0, s, 0.0, 0.0];
    let translate = [1.0, 0.0, 0.0, 1.0, t, t];
    mul(scale, translate)
}

/// `mul(a, b)` applies `b` and then `a`, matching the engine's own composition.
fn mul(a: Transform, b: Transform) -> Transform {
    let [a1, b1, c1, d1, e1, f1] = a;
    let [a2, b2, c2, d2, e2, f2] = b;
    [
        a1 * a2 + c1 * b2,
        b1 * a2 + d1 * b2,
        a1 * c2 + c1 * d2,
        b1 * c2 + d1 * d2,
        a1 * e2 + c1 * f2 + e1,
        b1 * e2 + d1 * f2 + f1,
    ]
}

/// Where the layout pass says a point lands, once the origin is baked in.
fn through(m: Transform, x: f32, y: f32) -> (f32, f32) {
    let [a, b, c, d, e, f] = m;
    (a * x + c * y + e, b * x + d * y + f)
}

/// Lay out one `n`-pixel box carrying `transform`, and return the matrix the
/// engine actually pushed, origin already baked.
fn pushed(n: f32, transform: Transform) -> Transform {
    let mut root = Node::new(Style {
        display: Display::Flex,
        axis: Axis::Column,
        ..Default::default()
    });
    root.children.push(Node::new(Style {
        width: Some(Len::Px(n)),
        height: Some(Len::Px(n)),
        transform: Some(transform),
        ..Default::default()
    }));
    let mut measure = |_: &TextContent, _: Option<f32>| (0.0, 0.0);
    let out = layout(&root, 1000.0, 800.0, &mut measure);
    out.paints
        .iter()
        .find_map(|p| match p {
            Paint::PushTransform(m) => Some(*m),
            _ => None,
        })
        .expect("no transform was pushed")
}

/// Within a twentieth of a pixel, which is well under anything visible.
fn near(got: (f32, f32), want: (f32, f32), what: &str) {
    assert!(
        (got.0 - want.0).abs() < 0.05 && (got.1 - want.1).abs() < 0.05,
        "{what}: got {got:?}, want {want:?}"
    );
}

#[test]
fn a_24_grid_fills_a_16px_box() {
    // 16px is an icon beside body text, which is the case that matters most.
    let m = pushed(16.0, icon_transform(16.0));
    near(through(m, 0.0, 0.0), (0.0, 0.0), "the grid's corner is the box's corner");
    near(through(m, GRID, GRID), (16.0, 16.0), "the grid's far corner is the box's");
    near(through(m, GRID / 2.0, GRID / 2.0), (8.0, 8.0), "the centre stays the centre");
}

#[test]
fn the_same_formula_holds_at_every_size() {
    // A formula that only works at one size is a constant that happens to fit.
    for n in [12.0_f32, 16.0, 20.0, 24.0, 32.0, 48.0, 64.0] {
        let m = pushed(n, icon_transform(n));
        near(through(m, 0.0, 0.0), (0.0, 0.0), &format!("corner at {n}px"));
        near(through(m, GRID, GRID), (n, n), &format!("far corner at {n}px"));
    }
}

#[test]
fn at_the_grids_own_size_the_transform_is_the_identity() {
    // 24px on a 24 grid should move nothing at all. If this one drifts, the
    // scale and the translate are disagreeing about direction.
    let m = pushed(GRID, icon_transform(GRID));
    near(through(m, 0.0, 0.0), (0.0, 0.0), "corner");
    near(through(m, GRID, GRID), (GRID, GRID), "far corner");
    near(through(m, 7.0, 19.0), (7.0, 19.0), "an arbitrary interior point");
}

#[test]
fn scaling_alone_does_not_do_it_and_this_is_why_the_translate_is_there() {
    // The negative result that justifies the whole formula. Without the
    // translate, the drawing is centred on the box centre but sized from it,
    // so a 24 grid in a 16px box overhangs at both ends.
    let s = 16.0 / GRID;
    let m = pushed(16.0, [s, 0.0, 0.0, s, 0.0, 0.0]);
    let corner = through(m, 0.0, 0.0);
    assert!(
        (corner.0 - 0.0).abs() > 0.5,
        "scale alone happened to land on the corner: {corner:?}"
    );
}
