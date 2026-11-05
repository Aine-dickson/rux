//! The root's background is the window's: it must follow a `:class` change
//! made in the same handler that removes the focused field (Tracer's AW-043).

use rux_layout::{Background, Node, Rgba};
use rux_runtime::Document;
use rux_style::InteractionState;

const SRC: &str = r#"<template>
  <screen class="root" :class="{ on: on }">
    <view class="child"><text>child</text></view>
    <view class="scrim" r-if="open">
      <input r-model="typed" autofocus />
    </view>
  </screen>
</template>
<style>
  .root { background: #202020; }
  .root.on { background: #f0f0f0; }
  .child { background: #202020; }
  .root.on .child { background: #f0f0f0; }
</style>
<script>
  let on = signal(true);
  let open = signal(false);
  let typed = signal("");
  fn flip() { on = !on; open = false; }
</script>"#;

fn canvas(root: &Node) -> f32 {
    root.style.rare.as_ref().and_then(|r| r.canvas).expect("the root has a canvas").r
}

fn background(node: &Node) -> f32 {
    match &node.style.background {
        Some(Background::Color(Rgba { r, .. })) => *r,
        other => panic!("{other:?}"),
    }
}

fn child(root: &Node) -> &Node {
    &root.children[0]
}

const LIGHT: f32 = 0xf0 as f32 / 255.0;
const DARK: f32 = 0x20 as f32 / 255.0;

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

#[test]
fn the_root_follows_its_class_as_the_focused_field_goes() {
    let mut d = Document::from_source(SRC).expect("loads");
    let mut want = LIGHT;
    assert!(close(canvas(&d.root), want));
    for round in 0..3 {
        d.apply_handler_in("open = true", None);
        // The field takes focus, as `autofocus` does in the shell.
        d.set_focus(Some(rux_runtime::Focus {
            model: "typed".into(),
            row: None,
            instance: None,
            caret: 0,
            anchor: 0,
            preedit: None,
        }));
        d.set_interaction(InteractionState {
            focused_model: Some("typed".into()),
            ..InteractionState::default()
        });
        d.apply_handler_in("flip()", None);
        let flipped = if want == LIGHT { DARK } else { LIGHT };
        assert!(close(canvas(&d.root), flipped), "round {round}: after the handler, the root's canvas is {}", canvas(&d.root));
        // The shell finds the field gone and drops the focus, the field now
        // touched, as it records one being left.
        d.set_interaction(InteractionState {
            touched: vec![("typed".into(), None, None)],
            ..InteractionState::default()
        });
        want = if want == LIGHT { DARK } else { LIGHT };
        assert!(close(background(child(&d.root)), want), "round {round}: the child");
        assert!(close(canvas(&d.root), want), "round {round}: the root's canvas is {}", canvas(&d.root));
    }
}
