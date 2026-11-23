//! A scroller's position, read and set from script (Urbandrop's UD-014).
//!
//! `el.scrollLeft` and friends come from what the shell hands the document
//! each frame (`set_scroll_regions`), the same way a box's `x` and `width`
//! come from `set_metrics`; `el.scrollTo` is queued for the shell to apply.

use rux_runtime::{Document, ScrollSnapshot};

fn measure(tc: &rux_layout::TextContent, max: Option<f32>) -> (f32, f32) {
    let w = tc.text.chars().count() as f32 * 7.0;
    let cap = max.unwrap_or(f32::INFINITY);
    let lines = (w / cap).ceil().max(1.0);
    (w.min(cap), lines * 18.0)
}

fn document() -> Document {
    Document::from_source(
        "<template><screen>\
           <view class=\"list\">\
             <view class=\"row\"><text>one</text></view>\
             <view class=\"row\"><text>two</text></view>\
             <view class=\"row\"><text>three</text></view>\
             <view class=\"row\"><text>four</text></view>\
             <view class=\"row\"><text>five</text></view>\
           </view>\
           <text>{{ report }}</text>\
         </screen></template>\n\
         <style>\n.list { max-height: 50px; overflow-y: auto; }\n.row { height: 30px; }\n</style>\n\
         <script>\n\
         let report = signal(\"none\");\n\
         fn read() {\n\
           let list = query(\".list\")[0];\n\
           report = \"top \" + list.scrollTop + \" of \" + list.scrollHeight + \" in \" + list.clientHeight + \" left \" + list.scrollLeft;\n\
         }\n\
         fn read_row() {\n\
           let row = query(\".row\")[0];\n\
           report = \"row \" + row.scrollTop + \" \" + row.scrollHeight + \" \" + row.clientHeight;\n\
         }\n\
         fn go() { query(\".list\")[0].scrollTo(0, 40); }\n\
         fn glide() { query(\".list\")[0].scrollTo(5, 60, { smooth: true }); }\n\
         fn bad() { query(\".list\")[0].scrollTo(0, 40, { fast: true }); }\n\
         </script>",
    )
    .expect("loads")
}

fn text_of(doc: &Document) -> String {
    fn all(node: &rux_layout::Node, out: &mut Vec<String>) {
        out.extend(node.text.iter().map(|t| t.text.clone()));
        for c in &node.children {
            all(c, out);
        }
    }
    let mut out = Vec::new();
    all(&doc.root, &mut out);
    out.into_iter().find(|t| t.contains("top ") || t.starts_with("row ")).unwrap_or_default()
}

/// Lay out, and hand the document the metrics and the scroller's state, as the
/// shell does each frame.
fn frame(doc: &mut Document, scrolled_to: f32) {
    let mut m = measure;
    let layout = rux_layout::layout(&doc.root, 400.0, 600.0, &mut m);
    assert_eq!(layout.scrolls.len(), 1, "the capped list scrolls: {:?}", layout.scrolls);
    let r = &layout.scrolls[0];
    doc.set_metrics(layout.metrics.clone());
    doc.set_scroll_regions(vec![ScrollSnapshot {
        x: r.x,
        y: r.y,
        width: r.width,
        height: r.height,
        left: 0.0,
        top: scrolled_to,
        content_width: r.content_width,
        content_height: r.content_height,
    }]);
}

#[test]
fn a_scroller_reads_where_it_is_and_how_big_it_is() {
    let mut doc = document();
    frame(&mut doc, 40.0);
    assert!(doc.apply_handler("read()"));
    let said = text_of(&doc);
    assert!(said.starts_with("top 40 of 150 in 50 left 0"), "{said}");
}

#[test]
fn a_box_that_does_not_scroll_reads_as_not_scrolled_with_its_own_size() {
    let mut doc = document();
    frame(&mut doc, 40.0);
    assert!(doc.apply_handler("read_row()"));
    let said = text_of(&doc);
    assert_eq!(said, "row 0 30 30", "a row is not a scroller");
}

#[test]
fn scroll_to_is_queued_for_the_shell_with_where_and_how() {
    let mut doc = document();
    frame(&mut doc, 0.0);
    assert!(doc.apply_handler("go()"));
    let asked = doc.take_scrolls();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!((asked[0].x, asked[0].y, asked[0].by, asked[0].smooth), (0.0, 40.0, false, false));

    assert!(doc.apply_handler("glide()"));
    let asked = doc.take_scrolls();
    assert_eq!((asked[0].x, asked[0].y, asked[0].smooth), (5.0, 60.0, true));
}

#[test]
fn scroll_to_with_an_option_it_does_not_have_says_so() {
    let mut doc = document();
    frame(&mut doc, 0.0);
    doc.apply_handler("bad()");
    assert!(doc.take_scrolls().is_empty(), "nothing queued for a refused call");
    let warned = rux_runtime::take_warnings();
    assert!(warned.iter().any(|w| w.message.contains("no option `fast`")), "{warned:?}");
}

#[test]
fn scroll_and_scrollend_are_kept_on_the_node_with_a_typed_event() {
    let doc = Document::from_source(
        "<template><screen>\
           <view class=\"list\" @scroll=\"n = event.scrollTop\" @scrollend=\"done = event.maxTop\">\
             <view class=\"row\"><text>one</text></view>\
           </view>\
         </screen></template>\n\
         <style>\n.list { max-height: 50px; overflow-y: auto; }\n</style>\n\
         <script>\nlet n = signal(0.0);\nlet done = signal(0.0);\n</script>",
    )
    .expect("loads");
    let list = &doc.root.children[0];
    let events = list.scroll_events().expect("the list's handlers are kept");
    assert_eq!(events.scroll.as_deref(), Some("n = event.scrollTop"));
    assert_eq!(events.end.as_deref(), Some("done = event.maxTop"));
    assert!(rux_runtime::take_warnings().is_empty(), "no complaint about the new events");
}
