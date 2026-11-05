//! A `computed` read in the handler that changed what it reads gives the new
//! value, as a getter would, not the one from before the handler (Tracer's
//! AW-038).

use rux_runtime::Document;

fn texts(n: &rux_layout::Node) -> Vec<String> {
    let mut out: Vec<String> = n.text.iter().map(|t| t.text.trim().to_string()).collect();
    for c in &n.children {
        out.extend(texts(c));
    }
    out
}

fn tap(doc: &mut Document, label: &str) {
    fn find<'a>(n: &'a rux_layout::Node, l: &str) -> Option<&'a rux_layout::Node> {
        if n.on_tap.is_some() && texts(n).iter().any(|t| t == l) {
            return Some(n);
        }
        n.children.iter().find_map(|c| find(c, l))
    }
    let node = find(&doc.root, label).unwrap_or_else(|| panic!("nothing says {label}")).clone();
    assert!(doc.apply_handler_in(&node.on_tap.clone().unwrap(), node.instance.as_deref()));
}

fn shown(doc: &Document) -> String {
    texts(&doc.root).join("|")
}

#[test]
fn a_computed_read_after_a_write_in_the_same_handler_is_current() {
    let src = "<template><screen>\
          <view @tap=\"go()\"><text>go</text></view>\
          <view @tap=\"again()\"><text>again</text></view>\
          <text>a = {{ a }}, b = {{ b }}, c = {{ c }}</text>\
          <text>seen {{ seen }}</text>\
        </screen></template>\n\
        <script>\n\
        let a = signal(1);\n\
        computed b = a * 2;\n\
        computed c = b + 1;\n\
        let seen = signal(\"not yet\");\n\
        fn go() { a = 5; seen = `b ${b} c ${c}`; }\n\
        fn again() { a += 1; let first = b; a += 1; seen = `${first} then ${b}`; }\n\
        </script>";
    let mut doc = Document::from_source(src).expect("loads");
    assert!(shown(&doc).contains("a = 1, b = 2, c = 3"), "{}", shown(&doc));
    tap(&mut doc, "go");
    let s = shown(&doc);
    assert!(s.contains("seen b 10 c 11"), "{s}");
    assert!(s.contains("a = 5, b = 10, c = 11"), "{s}");
    tap(&mut doc, "again");
    let s = shown(&doc);
    assert!(s.contains("seen 12 then 14"), "{s}");
    assert!(s.contains("a = 7, b = 14, c = 15"), "{s}");
}

/// A computed in a component is current in the component's handler too, in
/// a function it calls and in the handler's own text, through a chain, and
/// for each instance on its own.
#[test]
fn a_components_computed_is_current_in_its_handler() {
    let dir = std::env::temp_dir().join(format!("rux_computed_pull_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("components")).unwrap();
    std::fs::write(
        dir.join("components/card.rux"),
        "<template><view>\
           <view @tap=\"go()\"><text>go {{ label }}</text></view>\
           <view @tap=\"n += 1; seen = `inline ${twice} ${more}`\"><text>inline {{ label }}</text></view>\
           <text>{{ label }} seen {{ seen }}, shown {{ twice }} {{ more }}</text>\
         </view></template>\n\
         <script>\nprop label: string;\nlet n = signal(1);\ncomputed twice = n * 2;\ncomputed more = twice + 1;\nlet seen = signal(\"\");\n\
         fn go() { n = 4; seen = `twice ${twice} more ${more}`; }\n</script>",
    )
    .unwrap();
    std::fs::write(
        dir.join("app.rux"),
        "<template><screen><card :label='\"a\"' /><card :label='\"b\"' /></screen></template>\n<script>\nuse components::card;\n</script>",
    )
    .unwrap();
    let mut doc = Document::load(dir.join("app.rux")).expect("loads");
    tap(&mut doc, "go a");
    let s = shown(&doc);
    assert!(s.contains("a seen twice 8 more 9, shown 8 9"), "{s}");
    assert!(s.contains("b seen , shown 2 3"), "{s}");
    tap(&mut doc, "inline b");
    let s = shown(&doc);
    assert!(s.contains("b seen inline 4 5, shown 4 5"), "{s}");
    assert!(s.contains("a seen twice 8 more 9, shown 8 9"), "{s}");
}
