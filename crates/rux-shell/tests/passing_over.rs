//! A row the clips cut wholly away is passed over (`rux_layout::fold`): no
//! paint, no tap or hover regions. Proved here against the same document
//! laid out with nothing passed over: the paint the scene keeps is the same,
//! and so is every box's place for `query()`, at every scroll offset.

use rux_layout::{LayoutState, Offset, Paint};

fn lines(row_css: &str) -> String {
    format!(
        r#"<template>
  <screen class="app">
    <text class="h">{{{{ lines.length }}}} rows</text>
    <view class="viewer">
      <view class="ln" r-for="l in lines" r-key="l.no">
        <text class="no">{{{{ l.no }}}}</text>
        <text class="code" :style="`padding-left: ${{l.indent * 18}}px`"><text class="key">{{{{ l.key }}}}</text><text :class="l.kind">{{{{ l.value }}}}</text></text>
      </view>
    </view>
  </screen>
</template>

<style>
  .app {{ display: flex; flex-direction: column; gap: 14px; padding: 28px; height: 100vh; }}
  .viewer {{ flex: 1; min-height: 0; overflow-y: auto; padding: 8px 0; border: 1px solid #262626; }}
  .ln {{ display: flex; gap: 14px; padding: 0 12px; {row_css} }}
  .no {{ color: #555555; font-size: 13px; width: 48px; text-align: right; }}
  .code {{ color: #d4d4d4; font-size: 13px; }}
  .key {{ color: #9cdcfe; }}
  .string {{ color: #ce9178; }}
</style>

<script>
  type Line = {{ no: int, indent: int, key: string, value: string, kind: string }};
  let out: Line[] = [];
  for i in 0..300 {{
    out.push({{ no: i + 1, indent: i % 3, key: "\"field\": ", value: `"record ${{i}}",`, kind: "string" }});
  }}
  let lines: Line[] = signal(out);
</script>
"#
    )
}

/// Every paint the scene would draw, as text: the clips and filters around
/// them aside, which draw nothing themselves.
fn seen(paints: &[Paint], text: &mut rux_text::TextEngine) -> Vec<String> {
    let shown = rux_paint::visible_paints(paints, (800.0, 600.0), |tc, w| {
        text.measure(&tc.text, &rux_paint::text_style(tc), Some(w))
    });
    paints
        .iter()
        .zip(shown)
        .filter(|(p, keep)| *keep && !matches!(p, Paint::PushClip { .. } | Paint::PopClip | Paint::PushFilter { .. } | Paint::PopFilter))
        .map(|(p, _)| format!("{p:?}"))
        .collect()
}

fn compare(src: &str, label: &str) {
    let mut doc = rux_runtime::Document::from_source(src).expect("load");
    doc.fire_timers(0.0);
    let mut text = rux_text::TextEngine::new();
    let (mut passing, mut whole) = (LayoutState::new(), LayoutState::new());
    for f in 0..60 {
        let offsets = [Offset { x: 0.0, y: f as f32 * 97.0 }];
        rux_layout::set_passing_over(true);
        let a = passing.lay_out_text(&doc.root, 800.0, 600.0, &offsets, &mut rux_paint::EngineMeasure(&mut text));
        rux_layout::set_passing_over(false);
        let b = whole.lay_out_text(&doc.root, 800.0, 600.0, &offsets, &mut rux_paint::EngineMeasure(&mut text));
        rux_layout::set_passing_over(true);
        assert!(a.paints.len() < b.paints.len() || f == 0 && a.paints.len() == b.paints.len(), "{label}: rows passed over");
        let (sa, sb) = (seen(&a.paints, &mut text), seen(&b.paints, &mut text));
        if sa != sb {
            let only_a: Vec<_> = sa.iter().filter(|p| !sb.contains(p)).collect();
            let only_b: Vec<_> = sb.iter().filter(|p| !sa.contains(p)).collect();
            panic!("{label}: offset {f}: only passing over {only_a:#?}
only whole {only_b:#?}");
        }
        assert_eq!(a.metrics.len(), b.metrics.len(), "{label}: every box placed");
        for (m, n) in a.metrics.iter().zip(&b.metrics) {
            assert_eq!((&m.path, m.scroll), (&n.path, n.scroll), "{label}");
            let near = |p: f32, q: f32| (p - q).abs() < 1e-3;
            assert!(near(m.x, n.x) && near(m.y, n.y) && m.width == n.width && m.height == n.height, "{label}: {m:?} vs {n:?}");
        }
        assert_eq!(a.access.len(), b.access.len(), "{label}: every box read out");
        assert_eq!(a.contained, b.contained, "{label}");
    }
}

#[test]
fn a_sentence_per_row() {
    compare(&lines(""), "sentences");
}

#[test]
fn rows_with_shadows_and_outlines() {
    compare(&lines("box-shadow: 0 6px 12px #000; outline: 2px solid #fff; outline-offset: 3px;"), "shadows");
}

#[test]
fn rows_with_content_visibility() {
    compare(&lines("content-visibility: auto; contain-intrinsic-size: auto 20px;"), "content-visibility");
}

/// A text given less height than its lines spills out of the bottom of its
/// box, and is measured to know how far.
#[test]
fn text_spilling_out_of_its_box() {
    let src = lines("").replace(".code {", ".code { height: 3px; width: 60px; text-shadow: 0 4px 6px #000;");
    compare(&src, "spilling");
}
