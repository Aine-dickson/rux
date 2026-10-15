//! `filter` and `backdrop-filter`: what the painter draws apart for the
//! renderer, and in what order.

use rux_layout::filter::FilterOp;
use rux_layout::*;
use rux_paint::{build_scene, build_scene_apart, ImageCache};
use rux_text::TextEngine;

fn rect(r: f32) -> Paint {
    Paint::Rect(PaintRect {
        x: 0.0,
        y: 0.0,
        width: 10.0,
        height: 10.0,
        background: Some(Background::Color(Rgba::new(r, 0.0, 0.0, 1.0))),
        radius: [0.0; 4],
        border: Sides::default(),
        border_color: None,
    })
}

/// A filter inside a filter is made first, because the outer one's scene
/// draws its result; a backdrop is made where it is met; every image is
/// the window's size in device pixels.
#[test]
fn parts_drawn_apart_come_innermost_first() {
    let ops: std::sync::Arc<[FilterOp]> = vec![FilterOp::Blur(2.0)].into();
    let items = vec![
        rect(0.1),
        Paint::PushFilter { ops: ops.clone(), bounds: None },
        rect(0.2),
        Paint::PushFilter { ops: vec![FilterOp::Matrix(FilterOp::invert(1.0))].into(), bounds: None },
        rect(0.3),
        Paint::PopFilter,
        Paint::PopFilter,
        Paint::Backdrop { x: 0.0, y: 0.0, width: 10.0, height: 10.0, radius: [0.0; 4], ops },
    ];
    let (mut text, mut images) = (TextEngine::new(), ImageCache::new());
    let (_, jobs) = build_scene_apart(&items, &mut text, &mut images, false, (100.0, 50.0), 1.5);
    assert_eq!(jobs.len(), 3);
    assert!(matches!(jobs[0].ops[0], FilterOp::Matrix(_)), "the inner filter first");
    assert!(matches!(jobs[1].ops[0], FilterOp::Blur(_)) && !jobs[1].backdrop);
    assert!(jobs[2].backdrop);
    assert!(jobs.iter().all(|j| (j.image.width, j.image.height) == (150, 75)));
    // Each is a new image, so a renderer binds each to its own texture.
    assert_ne!(jobs[0].image.data.id(), jobs[1].image.data.id());

    // The plain builder draws the same items unfiltered, and nothing apart.
    let _ = build_scene(&items, &mut text, &mut images, false);
}
