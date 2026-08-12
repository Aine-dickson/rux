//! Transitions, driven end to end against the shipped example.
//!
//! The animator's own tests build a node by hand and step a fake clock. This
//! goes through the parts that actually have to agree with each other: the CSS
//! in `examples/transition.rux` is parsed, a real handler flips a real signal,
//! the document rebuilds, and only then does the animator see the change. A
//! transition that works in a unit test and not here is the interesting case,
//! and it is the one the shell would hit.

use std::path::{Path, PathBuf};

use rux_layout::{AnimProp, Node};
use rux_runtime::{Animator, Document};

fn example(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples").join(name)
}

/// The first node matching `pred`, depth-first.
fn find<'a>(node: &'a Node, pred: &dyn Fn(&Node) -> bool) -> Option<&'a Node> {
    if pred(node) {
        return Some(node);
    }
    node.children.iter().find_map(|c| find(c, pred))
}

/// The switch's knob: the only node that transitions its own `transform`.
fn knob(doc: &Document) -> &Node {
    find(&doc.root, &|n| {
        n.style.transitions.iter().any(|t| t.property == AnimProp::Transform)
            && n.children.is_empty()
    })
    .expect("the example has a knob")
}

/// How far the knob is translated along x.
fn slid(doc: &Document) -> f32 {
    knob(doc).style.transform.map_or(0.0, |m| m[4])
}

#[test]
fn the_switch_slides_instead_of_jumping() {
    let mut doc = Document::load(example("transition.rux")).expect("loads");
    let mut anim = Animator::new();

    // First frame: the knob is where the stylesheet puts it, and nothing is
    // animating, so the app is free to sleep.
    assert_eq!(anim.apply(&mut doc.root, 0.0), None);
    assert_eq!(slid(&doc), 0.0);

    // Tap the switch. The build moves the target to translateX(26px); the frame
    // it happens on still draws the knob at 0, because that is where it is.
    assert!(doc.apply_handler("on = !on"), "the tap changed state");
    assert_eq!(slid(&doc), 26.0, "the build moved the target");
    assert!(anim.apply(&mut doc.root, 1_000.0).is_some(), "a frame is now due");
    assert_eq!(slid(&doc), 0.0, "the knob has not moved yet");

    // Mid-flight (the example says 180ms): somewhere strictly between the two,
    // which is the whole claim being made.
    anim.apply(&mut doc.root, 1_090.0);
    let mid = slid(&doc);
    assert!(mid > 0.0 && mid < 26.0, "knob mid-slide, got {mid}");

    // And it arrives, exactly, at its own 180ms. The document is still asking
    // for frames here, because the same tap opened the panel over 250ms: each
    // property runs on its own clock, which is the point of naming them
    // separately.
    anim.apply(&mut doc.root, 1_180.0);
    assert_eq!(slid(&doc), 26.0);

    // Once the longest of them lands, the whole document goes quiet.
    assert_eq!(anim.apply(&mut doc.root, 1_250.0), None);
    assert!(anim.is_idle());

    // Idle really is idle: further frames change nothing and schedule nothing.
    assert_eq!(anim.apply(&mut doc.root, 5_000.0), None);
    assert_eq!(slid(&doc), 26.0);
}

#[test]
fn a_document_with_no_transitions_never_asks_for_a_frame() {
    // The property that matters on a phone: adding an animator must not turn an
    // event-driven app into one that renders continuously.
    let mut doc = Document::load(example("counter.rux")).expect("loads");
    let mut anim = Animator::new();
    for frame in 0..10 {
        assert_eq!(anim.apply(&mut doc.root, frame as f64 * 16.0), None);
    }
    assert!(doc.apply_handler("n = n + 1"), "the tap changed state");
    assert_eq!(anim.apply(&mut doc.root, 200.0), None, "a plain state change is still free");
}
