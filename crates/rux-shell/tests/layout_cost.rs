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
    cost(LIST, "list 300 rows");
}

/// The same list with every row `position: relative`, so every row is held
/// back by its stacking context and drawn after the in-flow content, with the
/// clips it sits in put back around it: the most a context can be asked to
/// sort in one frame.
#[test]
#[ignore]
fn positioned_list_layout_cost() {
    let src = LIST.replace(".row { display: flex;", ".row { position: relative; display: flex;");
    assert_ne!(src, LIST);
    cost(&src, "list 300 positioned rows");
}

/// The positioned list with every row's button an anchor and a `fixed` menu
/// on one of them, so the paint walk runs twice: once to find where the
/// anchors are drawn and once to draw against them.
#[test]
#[ignore]
fn anchored_list_layout_cost() {
    let src = LIST
        .replace(".row { display: flex;", ".row { position: relative; display: flex;")
        .replace(
            r#"<text class="price">"#,
            r#"<text class="price" :style="`anchor-name: --row-${item.id}`">"#,
        )
        .replace(
            "  </screen>",
            "    <view class=\"menu\"><text class=\"name\">menu</text></view>\n  </screen>",
        )
        .replace(
            "</style>",
            ".menu { position: fixed; position-anchor: --row-150; top: anchor(bottom); left: anchor(left); \
             width: 120px; background: #fab387; }\n</style>",
        );
    assert!(src.contains("position-anchor") && src.contains("anchor-name"));
    cost(&src, "list 300 anchored rows");
}

fn cost(src: &str, label: &str) {
    let mut doc = rux_runtime::Document::from_source(src).expect("load");
    let mut text = rux_text::TextEngine::new();
    let mut now = 0.0;
    let frames = 200;
    let mut layout_us = Vec::with_capacity(frames);
    let mut patch_us = Vec::with_capacity(frames);
    let mut scene_us = Vec::with_capacity(frames);
    let mut images = rux_paint::ImageCache::new();
    let mut kept = rux_layout::LayoutState::new();
    for _ in 0..frames {
        now += 16.0;
        let t = Instant::now();
        doc.fire_timers(now);
        patch_us.push(t.elapsed().as_secs_f64() * 1e6);
        let t = Instant::now();
        let mut measure = |tc: &rux_layout::TextContent, mw: Option<f32>| {
            text.measure(&tc.text, &rux_paint::text_style(tc), mw)
        };
        let out = kept.layout_scrolled(&doc.root, 800.0, 600.0, &[], &mut measure);
        layout_us.push(t.elapsed().as_secs_f64() * 1e6);
        // A kept layout is the whole one, checked outside the timing. Debug
        // builds check every frame inside `layout_scrolled` itself.
        if layout_us.len() % 10 == 0 {
            let whole = rux_layout::layout(&doc.root, 800.0, 600.0, &mut measure);
            assert_eq!(format!("{out:?}"), format!("{whole:?}"), "kept and whole layouts differ");
        }
        let t = Instant::now();
        let scene = rux_paint::build_scene_within(&out.paints, &mut text, &mut images, false, (800.0, 600.0));
        scene_us.push(t.elapsed().as_secs_f64() * 1e6);
        std::hint::black_box(scene);
    }
    // The first frames warm the measure cache; report the steady state.
    let steady = |v: &mut Vec<f64>| {
        let mut s: Vec<f64> = v[20..].to_vec();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap());
        (s[s.len() / 2], s[0])
    };
    eprintln!("reconciles that built only the list: {} of {frames}", doc.lists_built_alone());
    let (lm, lb) = steady(&mut layout_us);
    let (pm, pb) = steady(&mut patch_us);
    let (sm, sb) = steady(&mut scene_us);
    eprintln!("{label}: layout median {lm:.0} us (best {lb:.0}); timer+patch median {pm:.0} us (best {pb:.0}); scene median {sm:.0} us (best {sb:.0})");
}
