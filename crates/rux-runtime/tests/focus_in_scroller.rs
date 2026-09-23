//! Which scrollers hold a focusable, recorded by the layout rather than
//! guessed by the shell.
//!
//! Focusing an element scrolls it into view, and only the scrollers it is
//! inside should move. The shell used to guess them from geometry, any
//! scroller the element overlapped sideways, so a button above a list
//! scrolled the list back to its top when Shift+Tab reached it. The chain is
//! `FocusItem::scroll` and then each `ScrollRegion::within`, pinned here; the
//! shell only walks it.

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
           <view @tap=\"a = 1\"><text>outside, and as wide as the list</text></view>\
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
    assert_eq!(layout.focusables[0].scroll, None, "above the list, not in it");
    assert_eq!(layout.focusables[1].scroll, Some(layout.scrolls[0].id), "in the list");
}

/// A scroller that is itself focusable is held by its *parent*, not by
/// itself: it does not scroll to show itself.
#[test]
fn a_scroller_is_not_its_own_holder() {
    let doc = Document::from_source(
        "<template><screen>\
           <view class=\"list\" @tap=\"a = 1\"><text>a long label that will wrap and overflow</text></view>\
         </screen></template>\n\
         <style>\n.list { max-height: 30px; overflow-y: auto; }\n</style>\n\
         <script>\nlet a = signal(0);\n</script>",
    )
    .expect("loads");
    let layout = layout_of(&doc);
    assert_eq!(layout.scrolls.len(), 1);
    assert_eq!(layout.focusables[0].scroll, None);
}

/// A list inside a scrolling page: the row is held by the list, the list by
/// the page, and the page by nothing. Outer ones are recorded first.
#[test]
fn nested_scrollers_record_the_chain() {
    let doc = Document::from_source(
        "<template><screen class=\"page\">\
           <view class=\"list\">\
             <view class=\"row\" @tap=\"a = 1\"><text>row</text></view>\
             <view class=\"row\"><text>filler</text></view>\
           </view>\
           <view class=\"tall\"></view>\
         </screen></template>\n\
         <style>\n.page { height: 600px; overflow-y: auto; }\n\
         .list { height: 40px; overflow-y: auto; }\n.row { height: 30px; }\n\
         .tall { height: 2000px; }\n</style>\n\
         <script>\nlet a = signal(0);\n</script>",
    )
    .expect("loads");
    let layout = layout_of(&doc);
    assert_eq!(layout.scrolls.len(), 2, "{:?}", layout.scrolls);
    let (page, list) = (&layout.scrolls[0], &layout.scrolls[1]);
    assert_eq!(page.within, None, "the page is held by nothing");
    assert_eq!(list.within, Some(page.id), "the list is held by the page");
    assert_eq!(layout.focusables[0].scroll, Some(list.id), "the row by the list");
}
