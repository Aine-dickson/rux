//! What a frame's layout costs, measured outside the window: track (g) of
//! `docs/11-next.md` item 10. Ignored by default; run it in release:
//!
//! `cargo test -p rux-shell --release --test layout_cost -- --ignored --nocapture`
//!
//! The document is the script-cost list (300 keyed rows, three texts each),
//! rotated one row a frame by its own timer, laid out and drawn into a scene
//! with the real text engine as the shell does.

use std::time::Instant;

const LIST: &str = r#"<template>
  <screen class="app">
    <text class="title">layout cost: list</text>
    <text class="line">tick {{ tick }}</text>
    <view class="rows">
      <view class="row" r-for="item in visible" r-key="item.id">
        <text class="id">{{ item.id }}</text>
        <text class="name">{{ item.name }}</text>
        <text class="price">{{ item.price * item.qty }}</text>
      </view>
    </view>
  </screen>
</template>

<style>
  .app { display: flex; flex-direction: column; gap: 8px; padding: 16px; background: #1e1e2e; }
  .title { color: #cdd6f4; font-size: 20px; font-weight: 700; }
  .line { color: #9399b2; font-size: 13px; }
  .rows { display: flex; flex-direction: column; gap: 2px; }
  .row { display: flex; gap: 12px; padding: 4px 8px; background: #313244; border-radius: 6px; }
  .id { color: #89b4fa; width: 48px; font-size: 12px; }
  .name { color: #cdd6f4; width: 120px; font-size: 12px; }
  .price { color: #a6e3a1; font-size: 12px; }
</style>

<script>
  type Item = { id: int, name: string, price: float, qty: int };

  let tick = signal(0);
  let visible: Item[] = signal([]);

  fn make(n: int): Item[] {
    let out: Item[] = [];
    for i in 0..n {
      out.push({ id: i, name: `item ${i}`, price: (i % 97) + 0.5, qty: (i % 7) + 1 });
    }
    out
  }

  mounted {
    visible = make(300);
    setInterval(16) {
      tick++;
      let first = visible[0];
      let rest = visible.slice(1, visible.length);
      rest.push(first);
      visible = rest;
    }
  }
</script>
"#;

#[test]
#[ignore]
fn list_layout_cost() {
    let mut doc = rux_runtime::Document::from_source(LIST).expect("load");
    let mut text = rux_text::TextEngine::new();
    let mut now = 0.0;
    let frames = 200;
    let mut layout_us = Vec::with_capacity(frames);
    let mut patch_us = Vec::with_capacity(frames);
    let mut scene_us = Vec::with_capacity(frames);
    let mut images = rux_paint::ImageCache::new();
    for _ in 0..frames {
        now += 16.0;
        let t = Instant::now();
        doc.fire_timers(now);
        patch_us.push(t.elapsed().as_secs_f64() * 1e6);
        let t = Instant::now();
        let mut measure = |tc: &rux_layout::TextContent, mw: Option<f32>| {
            text.measure(&tc.text, &rux_paint::text_style(tc), mw)
        };
        let out = rux_layout::layout(&doc.root, 800.0, 600.0, &mut measure);
        layout_us.push(t.elapsed().as_secs_f64() * 1e6);
        let t = Instant::now();
        let scene = rux_paint::build_scene(&out.paints, &mut text, &mut images, false);
        scene_us.push(t.elapsed().as_secs_f64() * 1e6);
        std::hint::black_box(scene);
    }
    // The first frames warm the measure cache; report the steady state.
    let steady = |v: &mut Vec<f64>| {
        let mut s: Vec<f64> = v[20..].to_vec();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        (s[s.len() / 2], s[0])
    };
    let (lm, lb) = steady(&mut layout_us);
    let (pm, pb) = steady(&mut patch_us);
    let (sm, sb) = steady(&mut scene_us);
    eprintln!("list 300 rows: layout median {lm:.0} us (best {lb:.0}); timer+patch median {pm:.0} us (best {pb:.0}); scene median {sm:.0} us (best {sb:.0})");
}
