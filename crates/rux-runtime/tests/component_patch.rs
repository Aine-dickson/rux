//! A component's own state is patched in place, as the document's signals
//! are, rather than rebuilding the document (Tracer's AW-012). Its own binary,
//! since it turns profiling on to count rebuilds.

use rux_runtime::Document;
use rux_script::profile::{self, Phase};

fn project(files: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rux_component_patch_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (rel, body) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    dir
}

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

fn rebuilds() -> u32 {
    profile::take().calls[Phase::Rebuild as usize]
}

#[test]
fn a_components_own_state_is_patched() {
    std::env::set_var("RUX_PROFILE", "1");
    let card = "<template><view>\
          <view @tap=\"count += 1\"><text>add</text></view>\
          <text>{{ label }} {{ count }} {{ twice }}</text>\
          <view @tap=\"open = !open\"><text>toggle</text></view>\
          <text r-if=\"open\">opened {{ count }}</text>\
          <view r-for=\"r in rows\" r-key=\"r\"><text>row {{ r }}</text></view>\
          <view @tap=\"rows.push(rows.length)\"><text>more</text></view>\
          <input r-model=\"note\" @input=\"typed += 1\" />\
          <text>typed {{ typed }}</text>\
        </view></template>\n\
        <script>\nprop label: string;\nlet count = signal(0);\nlet open = signal(false);\nlet rows: int[] = signal([0, 1]);\n\
        let note = signal(\"\");\nlet typed = signal(0);\ncomputed twice = count * 2;\n</script>";
    let app = "<template><screen><card :label='\"a\"' /><card :label='\"b\"' /></screen></template>\n\
        <script>\nuse components::card;\n</script>";
    let dir = project(&[("app.rux", app), ("components/card.rux", card)]);
    let mut doc = Document::load(dir.join("app.rux")).expect("loads");
    assert!(profile::active(), "profiling is on in this binary");
    let _ = rebuilds();

    tap(&mut doc, "add");
    let shown = texts(&doc.root);
    assert!(shown.contains(&"a 1 2".to_string()), "the first card counted: {shown:?}");
    assert!(shown.contains(&"b 0 0".to_string()), "the second did not: {shown:?}");
    tap(&mut doc, "add");
    assert!(texts(&doc.root).contains(&"a 2 4".to_string()), "{:?}", texts(&doc.root));

    tap(&mut doc, "toggle");
    assert!(texts(&doc.root).contains(&"opened 2".to_string()), "{:?}", texts(&doc.root));
    tap(&mut doc, "add");
    assert!(texts(&doc.root).contains(&"opened 3".to_string()), "{:?}", texts(&doc.root));
    tap(&mut doc, "more");
    assert!(texts(&doc.root).contains(&"row 2".to_string()), "{:?}", texts(&doc.root));
    assert_eq!(rebuilds(), 0, "none of that rebuilt the document");

    // Typing, with a handler beside it, keeps both.
    let input = |doc: &Document| {
        fn find(n: &rux_layout::Node) -> Option<&rux_layout::Node> {
            if n.model.as_deref() == Some("note") {
                return Some(n);
            }
            n.children.iter().find_map(find)
        }
        find(&doc.root).cloned().expect("an input")
    };
    for text in ["h", "hi"] {
        let field = input(&doc);
        doc.apply_edit_in("note", None, field.instance.as_deref(), text);
        let body = field.field().on_input.clone().unwrap();
        doc.apply_handler_with_event(&body, field.instance.as_deref(), &rux_reactive::Value::Text(text.into()));
        assert_eq!(doc.value_in("note", None, field.instance.as_deref()), text);
    }
    assert!(texts(&doc.root).contains(&"typed 2".to_string()), "{:?}", texts(&doc.root));
    assert_eq!(rebuilds(), 0, "typing did not rebuild either");
}
