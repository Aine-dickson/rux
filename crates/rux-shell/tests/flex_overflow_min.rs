//! Tracer's AW-036: a flex item that clips has an automatic minimum width of
//! 0, so an ellipsised title beside a fixed-width column shrinks inside the
//! row, and the column keeps its width.

const PAGE: &str = r#"<template>
  <screen>
    <view class="side">
      <view class="row"><text class="m">GET</text><text class="u">https://httpbin.org/response-headers?X-Long-Header=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa</text></view>
      <view class="row"><text class="m">GET</text><text class="u">https://short.test/x</text></view>
    </view>
  </screen>
</template>

<style>
  .side { width: 300px; padding: 8px; }
  .row { display: flex; gap: 8px; }
  .m { width: 64px; }
  .u { flex: 1; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
</style>
"#;

#[test]
fn a_clipping_flex_item_shrinks_below_its_text() {
    let doc = rux_runtime::Document::from_source(PAGE).expect("load");
    let mut text = rux_text::TextEngine::new();
    let layout = rux_layout::LayoutState::new().lay_out_text(&doc.root, 800.0, 600.0, &[], &mut rux_paint::EngineMeasure(&mut text));
    let side = layout.metrics.iter().find(|m| m.path.len() == 1).expect("the side");
    let inner_right = side.x + side.width - 8.0;
    let items: Vec<&rux_layout::NodeMetrics> = layout.metrics.iter().filter(|m| m.path.len() == 3).collect();
    assert_eq!(items.len(), 4);
    for pair in items.chunks(2) {
        let (m, u) = (pair[0], pair[1]);
        assert!((m.width - 64.0).abs() < 0.5, "the method column is {} wide, not 64", m.width);
        assert!(u.x + u.width <= inner_right + 0.5, "the url ends at {}, past the side's content edge {}", u.x + u.width, inner_right);
    }
    // Both urls take what is left of the row, whatever their text.
    assert!((items[1].width - items[3].width).abs() < 0.5, "{} against {}", items[1].width, items[3].width);
}
