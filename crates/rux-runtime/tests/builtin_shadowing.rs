//! A file's own name wins over a built-in module of the same name.
//!
//! `network`, `location`, `settings` and the rest arrive in a script without
//! an import, when it mentions `network.`. An app written before one of them
//! existed may already have a `network` of its own (Urbandrop's partner app:
//! a signal in a component, a computed in a page), and it must keep working.

use rux_runtime::Document;

fn texts(node: &rux_layout::Node, out: &mut Vec<String>) {
    if let Some(t) = &node.text {
        out.push(t.text.clone());
    }
    for child in &node.children {
        texts(child, out);
    }
}

fn shown(doc: &Document) -> String {
    let mut out = Vec::new();
    texts(&doc.root, &mut out);
    out.join("|")
}

#[test]
fn an_apps_own_network_is_not_the_built_in_module() {
    let app = "<template><screen><router><route path=\"/\" view=\"earnings\" /></router></screen></template>\n\
<script>\nuse pages::earnings;\n</script>";
    // `network.` inside a `fn`: a component's functions were scanned for it
    // without its `let`s and `computed`s, so the built-in was linked over them.
    let page = "<template><view><text>[{{ shown() }}]</text><owner-fields /></view></template>\n\
<script>\nuse components::owner_fields;\nlet pick = signal(\"MTN\");\ncomputed network = pick;\n\
fn shown(): string { return network.toLowerCase(); }\n</script>";
    let component = "<template><view><text>[{{ form() }}]</text></view></template>\n\
<script>\nlet network = signal(\"Airtel\");\nfn form(): string { return network.toLowerCase(); }\n</script>";
    let dir = std::env::temp_dir().join(format!("rux_builtin_shadowing_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (rel, body) in [("app.rux", app), ("pages/earnings.rux", page), ("components/owner_fields.rux", component)] {
        std::fs::create_dir_all(dir.join(rel).parent().unwrap()).unwrap();
        std::fs::write(dir.join(rel), body).unwrap();
    }
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let errors: Vec<String> =
        doc.diagnostics().warnings.iter().filter(|w| w.is_error()).map(|w| w.message.clone()).collect();
    assert!(errors.is_empty(), "{errors:?}");
    assert!(shown(&doc).contains("[mtn]"), "{}", shown(&doc));
    assert!(shown(&doc).contains("[airtel]"), "{}", shown(&doc));
}
