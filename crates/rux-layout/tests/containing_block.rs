//! `position` with CSS's meanings: which box an out-of-flow one is measured
//! against, and which values are containing blocks.
//!
//! The default used to be `relative`, which made *every* box a containing block
//! and so made "against the nearest positioned ancestor" and "against the
//! parent" the same sentence. An author who wrote `position: relative` on the
//! box they meant, as CSS requires, was ignored and got the right answer anyway.
//! Only a wrapper in between told the two apart, and then it was silent.

use rux_layout::*;

/// The cover is the only red box in each tree, so this is how it is picked out.
fn is_red(r: &PaintRect) -> bool {
    matches!(r.background, Some(Background::Color(c)) if c.r > 0.5)
}

fn covering(position: Position) -> Node {
    Node::new(Style {
        position,
        inset: [
            Some(Len::Px(0.0)),
            Some(Len::Px(0.0)),
            Some(Len::Px(0.0)),
            Some(Len::Px(0.0)),
        ],
        background: Some(Background::Color(Rgba::new(1.0, 0.0, 0.0, 1.0))),
        ..Default::default()
    })
}

/// screen > wrapper (200x100) > cover, all insets zero, in a 600x400 window.
///
/// The screen is not given a size: `layout` forces the root to the viewport, so
/// "the screen" and "the window" are the same box and 600x400 is both.
///
/// Returns the cover's painted rect, which is the red one.
fn cover_in(wrapper_position: Position, cover_position: Position, pad: f32) -> PaintRect {
    let mut screen = Node::new(Style {
        display: Display::Flex,
        axis: Axis::Column,
        padding: Sides::uniform(pad),
        ..Default::default()
    });
    let mut wrapper = Node::new(Style {
        display: Display::Flex,
        position: wrapper_position,
        width: Some(Len::Px(200.0)),
        height: Some(Len::Px(100.0)),
        ..Default::default()
    });
    wrapper.children.push(covering(cover_position));
    screen.children.push(wrapper);

    let mut measure = |_: &TextContent, _: Option<f32>| (0.0, 0.0);
    layout(&screen, 600.0, 400.0, &mut measure)
        .paints
        .into_iter()
        .find_map(|p| match p {
            Paint::Rect(r) if is_red(&r) => Some(r),
            _ => None,
        })
        .expect("the cover was painted")
}

/// The wrapper says `position: relative`, so it is the containing block and the
/// cover fills it. This is the case that worked before and still has to.
#[test]
fn a_relative_wrapper_is_the_containing_block() {
    let r = cover_in(Position::Relative, Position::Absolute, 0.0);
    assert_eq!((r.width, r.height), (200.0, 100.0), "sized to the wrapper");
}

/// The wrapper says nothing, so it is `static`, so it is **not** a containing
/// block and the cover skips it for the screen. This is the case that was
/// silently wrong: it used to come back 200x100.
#[test]
fn a_static_wrapper_is_skipped_for_the_screen() {
    let r = cover_in(Position::Static, Position::Absolute, 0.0);
    assert_eq!(
        (r.width, r.height),
        (600.0, 400.0),
        "sized to the screen, not to the box it happens to be written in"
    );
    assert_eq!((r.x, r.y), (0.0, 0.0), "and placed there too");
}

/// `fixed` goes to the window whatever is in between, including a containing
/// block that would have claimed an absolute box.
#[test]
fn fixed_passes_every_containing_block() {
    let r = cover_in(Position::Relative, Position::Fixed, 0.0);
    assert_eq!(
        (r.width, r.height),
        (600.0, 400.0),
        "the relative wrapper does not hold a fixed box"
    );
}

/// The containing block is the **padding box**, so padding on the ancestor does
/// not push an out-of-flow child inwards. Worth pinning down: it is the half of
/// the rule people expect to work the other way.
#[test]
fn padding_on_the_containing_block_does_not_inset_it() {
    let r = cover_in(Position::Static, Position::Absolute, 20.0);
    assert_eq!((r.x, r.y), (0.0, 0.0), "at the padding edge");
    assert_eq!((r.width, r.height), (600.0, 400.0));
}

/// A static box ignores its insets. That is the whole difference between
/// `static` and `relative`, and it is why `static` is worth having rather than
/// being a synonym.
#[test]
fn a_static_box_ignores_its_insets() {
    let offset = |position: Position| {
        let mut screen = Node::new(Style {
            display: Display::Flex,
            axis: Axis::Column,
            width: Some(Len::Px(600.0)),
            height: Some(Len::Px(400.0)),
            ..Default::default()
        });
        let shifted = Node::new(Style {
            position,
            inset: [Some(Len::Px(30.0)), None, None, Some(Len::Px(40.0))],
            width: Some(Len::Px(50.0)),
            height: Some(Len::Px(50.0)),
            background: Some(Background::Color(Rgba::new(1.0, 0.0, 0.0, 1.0))),
            ..Default::default()
        });
        screen.children.push(shifted);
        let mut measure = |_: &TextContent, _: Option<f32>| (0.0, 0.0);
        layout(&screen, 1000.0, 800.0, &mut measure)
            .paints
            .into_iter()
            .find_map(|p| match p {
                Paint::Rect(r) if is_red(&r) => Some((r.x, r.y)),
                _ => None,
            })
            .expect("painted")
    };
    assert_eq!(offset(Position::Static), (0.0, 0.0), "insets ignored");
    assert_eq!(offset(Position::Relative), (40.0, 30.0), "insets honored");
}

/// An out-of-flow box with **no** insets keeps its static position, so it stays
/// with its parent rather than being carried to a containing block: its static
/// position is defined by its parent's flow and moving it would lose exactly the
/// thing it is asking for.
///
/// This is what `:leave-to { position: absolute }` relies on to hold a departing
/// element where it stood, so it is the regression that would hurt most.
#[test]
fn no_insets_still_means_where_it_would_have_been() {
    let mut screen = Node::new(Style {
        display: Display::Flex,
        axis: Axis::Column,
        padding: Sides::uniform(10.0),
        width: Some(Len::Px(600.0)),
        height: Some(Len::Px(400.0)),
        ..Default::default()
    });
    let mut wrapper = Node::new(Style {
        display: Display::Flex,
        axis: Axis::Column,
        width: Some(Len::Px(200.0)),
        height: Some(Len::Px(200.0)),
        ..Default::default()
    });
    wrapper.children.push(Node::new(Style {
        width: Some(Len::Px(50.0)),
        height: Some(Len::Px(60.0)),
        ..Default::default()
    }));
    let leaving = Node::new(Style {
        position: Position::Absolute,
        width: Some(Len::Px(50.0)),
        height: Some(Len::Px(50.0)),
        background: Some(Background::Color(Rgba::new(1.0, 0.0, 0.0, 1.0))),
        ..Default::default()
    });
    wrapper.children.push(leaving);
    screen.children.push(wrapper);

    let mut measure = |_: &TextContent, _: Option<f32>| (0.0, 0.0);
    let at = layout(&screen, 1000.0, 800.0, &mut measure)
        .paints
        .into_iter()
        .find_map(|p| match p {
            Paint::Rect(r) if is_red(&r) => Some((r.x, r.y)),
            _ => None,
        })
        .expect("painted");
    // 10px of screen padding, then below the 60px-tall sibling.
    assert_eq!(at, (10.0, 70.0), "where it would have been, not at an origin");
}

/// **A `transform` makes a containing block**, whatever the box's own
/// `position` says, and for `fixed` descendants as well as absolute ones.
///
/// This is CSS's rule and the reason `position: fixed` famously stops being
/// fixed inside a transformed parent. It is not an oddity to work around: a
/// transform moves the whole subtree, so there is no way to hold a descendant
/// still against the window while its ancestor slides, and pretending otherwise
/// would mean drawing it somewhere the transform says it is not.
#[test]
fn a_transform_is_a_containing_block_for_both() {
    let with_transform = |cover: Position| {
        let mut screen = Node::new(Style { display: Display::Flex, axis: Axis::Column, ..Default::default() });
        let mut wrapper = Node::new(Style {
            display: Display::Flex,
            // Says nothing about `position`, so it is `static`, and would be
            // skipped were it not for the transform.
            transform: Some([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
            width: Some(Len::Px(200.0)),
            height: Some(Len::Px(100.0)),
            ..Default::default()
        });
        wrapper.children.push(covering(cover));
        screen.children.push(wrapper);
        let mut measure = |_: &TextContent, _: Option<f32>| (0.0, 0.0);
        layout(&screen, 600.0, 400.0, &mut measure)
            .paints
            .into_iter()
            .find_map(|p| match p {
                Paint::Rect(r) if is_red(&r) => Some((r.width, r.height)),
                _ => None,
            })
            .expect("painted")
    };
    assert_eq!(with_transform(Position::Absolute), (200.0, 100.0), "absolute is claimed");
    assert_eq!(with_transform(Position::Fixed), (200.0, 100.0), "and so is fixed");
}
