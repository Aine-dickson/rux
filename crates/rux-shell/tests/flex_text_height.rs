//! Tracer's AW-035: text that wraps in a flex row whose other item shrinks.
//! The row must be as tall as the text is at the width it was given, not at
//! the width it asked for. Probe 2's own page and styles, measured by the
//! text engine as the shell lays out.

const PAGE: &str = r#"<template>
  <screen>
  <view class="page">
    <view class="viewer">
      <view class="ln" r-for="l in lines" r-key="l.no">
        <text class="no">{{ l.no }}</text>
        <text class="code" :style="`padding-left: ${l.indent * 18}px`"><text class="key">{{ l.key }}</text><text :class="l.kind">{{ l.value }}</text></text>
      </view>
    </view>
  </view>
  </screen>
</template>

<style>
  .page { display: flex; flex-direction: column; gap: 14px; padding: 28px; height: 100%; }
  .viewer { flex: 1; min-height: 0; overflow-y: auto; padding: 8px 0; border: 1px solid #262626; }
  .ln { display: flex; gap: 14px; padding: 0 12px; }
  .no { font-size: 13px; font-family: monospace; width: 48px; text-align: right; }
  .code { font-size: 13px; font-family: monospace; }
</style>

<script>
  let lines = [
    { no: 1, indent: 0, key: "", value: "[", kind: "punct" },
    { no: 2, indent: 1, key: "", value: "{", kind: "punct" },
    { no: 3, indent: 2, key: "\"id\": ", value: "1,", kind: "number" },
    { no: 4, indent: 2, key: "\"title\": ", value: "\"accusamus beatae ad facilis cum similique qui sunt\",", kind: "string" },
    { no: 5, indent: 2, key: "\"thumbnailUrl\": ", value: "\"https://via.placeholder.com/150/92c952 and some more words to wrap it here\",", kind: "string" },
    { no: 6, indent: 2, key: "\"url\": ", value: "\"https://via.placeholder.com/600/92c952 with a tail of words that is long enough to wrap twice over at this width\"", kind: "string" },
    { no: 7, indent: 1, key: "", value: "},", kind: "punct" },
    { no: 8, indent: 2, key: "\"title\": ", value: "\"record number 4 of 100\",", kind: "string" },
    { no: 9, indent: 2, key: "\"url\": ", value: "\"https://example.test/600/000004\",", kind: "string" },
    { no: 10, indent: 2, key: "\"score\": ", value: "-0.335,", kind: "number" },
  ];
</script>
"#;

fn rows_hold_their_text(width: f32) {
    let doc = rux_runtime::Document::from_source(PAGE).expect("load");
    let mut text = rux_text::TextEngine::new();
    let layout = rux_layout::LayoutState::new().lay_out_text(&doc.root, width, 700.0, &[], &mut rux_paint::EngineMeasure(&mut text));
    let rows: Vec<&rux_layout::NodeMetrics> = layout.metrics.iter().filter(|m| m.path.len() == 3).collect();
    assert_eq!(rows.len(), 10, "the rows, at {width}");
    // The url row's text is wider than its column at these widths, so it
    // wraps: two lines, and the row holds both.
    if (430.0..470.0).contains(&width) {
        let url = rows[8];
        assert!(url.height > 20.0, "at {width}, the url row is {} tall: one line for two", url.height);
    }
    for row in &rows {
        for child in layout.metrics.iter().filter(|m| m.path.len() == 4 && m.path[..3] == row.path[..]) {
            assert!(
                child.y + child.height <= row.y + row.height + 0.5,
                "at {width}, row {:?} is {} tall and its child {:?} is {} wide and {} tall, reaching below it",
                row.path,
                row.height,
                child.path,
                child.width,
                child.height
            );
        }
    }
    for pair in rows.windows(2) {
        assert!(pair[1].y + 0.5 >= pair[0].y + pair[0].height, "at {width}, {:?} overlaps the row above", pair[1].path);
    }
}

#[test]
fn a_flex_row_is_as_tall_as_its_wrapped_text() {
    for width in [667.0, 600.0, 520.0, 460.0, 440.0, 420.0, 400.0, 380.0] {
        rows_hold_their_text(width);
    }
}
