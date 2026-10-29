//! A row scrolled out of its list is not under the pointer.

use rux_runtime::Document;

fn measure(tc: &rux_layout::TextContent, max: Option<f32>) -> (f32, f32) {
    let w = tc.text.chars().count() as f32 * 7.0;
    (w.min(max.unwrap_or(f32::INFINITY)), 18.0)
}

#[test]
fn a_row_scrolled_up_under_the_header_does_not_take_its_tap() {
    let doc = Document::from_source(
        "<template><screen>\
           <view class=\"head\" @tap=\"a = 1\"><text>header</text></view>\
           <view class=\"list\">\
             <view class=\"row\" @tap=\"a = 2\"><text>row one</text></view>\
             <view class=\"row\" @tap=\"a = 3\"><text>row two</text></view>\
             <view class=\"row\" @tap=\"a = 4\"><text>row three</text></view>\
             <view class=\"row\" @tap=\"a = 5\"><text>row four</text></view>\
           </view>\
         </screen></template>\n\
         <style>\n.head { height: 40px; }\n.list { height: 60px; overflow-y: auto; }\n.row { height: 30px; flex-shrink: 0; }\n</style>\n\
         <script>\nlet a = signal(0);\n</script>",
    )
    .expect("loads");
    let mut m = measure;
    // Scrolled 60px: rows one and two have gone up out of the list, and
    // row two now lies over the bottom of the header.
    let layout = rux_layout::layout_scrolled(&doc.root, 400.0, 600.0, &[rux_layout::Offset { x: 0.0, y: 60.0 }], &mut m);
    let tap = rux_layout::tap_at(&layout.hits, 10.0, 20.0).and_then(|h| h.on_tap.clone());
    assert_eq!(tap.as_deref(), Some("a = 1"), "the header is what is under the pointer");
}

#[test]
fn a_row_scrolled_up_under_the_header_is_not_hovered() {
    let doc = Document::from_source(
        "<template><screen>\
           <view class=\"head\"><text>header</text></view>\
           <view class=\"list\">\
             <view class=\"row\"><text>row one</text></view>\
             <view class=\"row\"><text>row two</text></view>\
             <view class=\"row\"><text>row three</text></view>\
           </view>\
         </screen></template>\n\
         <style>\n.head { height: 40px; }\n.list { height: 60px; overflow-y: auto; }\n.row { height: 30px; flex-shrink: 0; }\n.row:hover { color: red; }\n</style>",
    )
    .expect("loads");
    let mut m = measure;
    let layout = rux_layout::layout_scrolled(&doc.root, 400.0, 600.0, &[rux_layout::Offset { x: 0.0, y: 60.0 }], &mut m);
    // Rows three and four fill the list, 40 to 100. Row two ends on its
    // top edge and row one above that: neither is there to hover.
    let seen: Vec<(f32, f32)> = layout.states.iter().map(|r| (r.y, r.height)).collect();
    assert_eq!(seen, [(40.0, 30.0), (70.0, 30.0)]);
}
