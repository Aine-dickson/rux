//! Tracer's AW-042: an `auto` margin in a flex container takes the free space
//! on its axis, and on the cross axis keeps the item from stretching. Before,
//! `auto` took no space, and the shorthand dropped it, so `margin: 0 auto`
//! was 0 all round.

const PAGE: &str = r#"<template>
  <screen>
    <view class="page">
      <view class="row"><text class="a">left</text><view class="push"><text>end</text></view></view>
      <view class="row"><view class="both"><text>centred</text></view></view>
      <view class="col"><view class="push"><text>column end</text></view></view>
      <view class="row"><view class="tb"><text>shorthand</text></view></view>
    </view>
  </screen>
</template>

<style>
  .page { display: flex; flex-direction: column; gap: 8px; padding: 8px; }
  .row { width: 400px; height: 60px; display: flex; gap: 8px; }
  .col { width: 400px; display: flex; flex-direction: column; }
  .a { width: 50px; }
  .push { margin-left: auto; width: 100px; }
  .both { margin: 0 auto; width: 100px; }
  .tb { margin: 5px auto; width: 100px; }
</style>
"#;

#[test]
fn auto_margins_take_the_free_space() {
    let doc = rux_runtime::Document::from_source(PAGE).expect("load");
    let mut text = rux_text::TextEngine::new();
    let layout = rux_layout::LayoutState::new().lay_out_text(&doc.root, 800.0, 600.0, &[], &mut rux_paint::EngineMeasure(&mut text));
    let at = |path: &[usize]| layout.metrics.iter().find(|m| m.path[..] == path[..]).unwrap_or_else(|| panic!("no box at {path:?}"));
    let near = |a: f32, b: f32| (a - b).abs() < 0.5;

    // A row: `margin-left: auto` puts the item at the row's right end.
    let row = at(&[0, 0]);
    let push = at(&[0, 0, 1]);
    assert!(near(push.x + push.width, row.x + row.width), "the pushed box ends at {}, the row at {}", push.x + push.width, row.x + row.width);

    // `margin: 0 auto` centres it.
    let row = at(&[0, 1]);
    let both = at(&[0, 1, 0]);
    assert!(near(both.x - row.x, row.x + row.width - (both.x + both.width)), "not centred: {} left, {} right", both.x - row.x, row.x + row.width - (both.x + both.width));

    // A column: the item keeps its own width and goes to the right.
    let col = at(&[0, 2]);
    let push = at(&[0, 2, 0]);
    assert!(near(push.width, 100.0), "the column item stretched to {}", push.width);
    assert!(near(push.x + push.width, col.x + col.width), "the column item ends at {}, the column at {}", push.x + push.width, col.x + col.width);

    // `margin: 5px auto`: 5px on top, centred across.
    let row = at(&[0, 3]);
    let tb = at(&[0, 3, 0]);
    assert!(near(tb.y - row.y, 5.0), "the top margin is {}, not 5", tb.y - row.y);
    assert!(near(tb.x - row.x, 150.0), "the left margin is {}, not 150", tb.x - row.x);
}
