//! A focusable inside a scrolling box has to know which box that is.
//!
//! The focus ring is painted by the shell as its own scene, after the
//! document's, so it never passes through the clip a scroller puts around its
//! children. `FocusItem::scroll` is what lets the shell put the clip back, and
//! it is filled in during layout, which is the part worth pinning here: the
//! shell's own tests cover what it then does with it.

use rux_runtime::Document;

fn measure(tc: &rux_layout::TextContent, max: Option<f32>) -> (f32, f32) {
    let w = tc.text.chars().count() as f32 * 7.0;
    let cap = max.unwrap_or(f32::INFINITY);
    let lines = (w / cap).ceil().max(1.0);
    (w.min(cap), lines * 18.0)
}

fn layout_of(doc: &Document) -> rux_layout::Layout {
    let mut m = measure;
    rux_layout::layout_scrolled(&doc.root, 400.0, 600.0, &[rux_layout::Offset::default(); 4], &mut m)
}

#[test]
fn a_focusable_records_the_scroller_it_sits_in() {
    let doc = Document::from_source(
        "<template><screen>\
           <view @tap=\"a = 1\"><text>outside</text></view>\
           <view class=\"list\">\
             <view @tap=\"a = 2\"><text>inside</text></view>\
           </view>\
         </screen></template>\n\
         <style>\n.list { max-height: 60px; overflow-y: auto; }\n</style>\n\
         <script>\nlet a = signal(0);\n</script>",
    )
    .expect("loads");

    let layout = layout_of(&doc);
    assert_eq!(layout.scrolls.len(), 1, "the capped box scrolls: {:?}", layout.scrolls);
    assert_eq!(layout.focusables.len(), 2, "both boxes tap, so both focus");

    // Tree order: the one written first is outside, the second is in the list.
    assert_eq!(layout.focusables[0].scroll, None, "nothing above it clips it");
    assert_eq!(
        layout.focusables[1].scroll,
        Some(layout.scrolls[0].id),
        "the one in the list is clipped by the list"
    );
}

/// A scroller that is itself focusable is clipped by its *parent*, not by
/// itself: its own clip bounds its children, and it is not one of them.
#[test]
fn a_scroller_is_not_its_own_clip() {
    let doc = Document::from_source(
        "<template><screen>\
           <view class=\"list\" @tap=\"a = 1\"><text>a long label that will wrap and overflow</text></view>\
         </screen></template>\n\
         <style>\n.list { max-height: 30px; overflow-y: auto; }\n</style>\n\
         <script>\nlet a = signal(0);\n</script>",
    )
    .expect("loads");

    let layout = layout_of(&doc);
    assert_eq!(layout.scrolls.len(), 1, "{:?}", layout.scrolls);
    assert_eq!(layout.focusables.len(), 1);
    assert_eq!(layout.focusables[0].scroll, None, "it is not inside itself");
}
