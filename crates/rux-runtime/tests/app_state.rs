//! `app.state`: a document reads whether the app is on screen without
//! importing anything, and follows what the shell reports.

use rux_runtime::native::{self as rn, Any, Call};
use rux_runtime::Document;

fn texts(n: &rux_layout::Node) -> String {
    n.text.iter().map(|t| t.text.clone()).chain(n.children.iter().map(texts)).collect()
}

#[test]
fn app_state_follows_the_shell_and_needs_no_import() {
    // What the shell installs at start-up.
    let mut calls = std::collections::HashMap::new();
    calls.insert("state".to_string(), Call::sync(|_args: Vec<Any>| rn::app::STATE.__read()));
    let _ = rn::install(rn::Module { interface: rux_runtime::builtins::app_interface(), calls });
    rn::app::set_visible(true);

    let dir = std::env::temp_dir().join(format!("rux_app_state_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("app.rux"),
        "<template><screen><text>now {{ app.state }}</text></screen></template>\n",
    )
    .unwrap();
    let mut doc = Document::load(dir.join("app.rux")).expect("loads");
    assert!(texts(&doc.root).contains("now foreground"), "{}", texts(&doc.root));

    rn::app::set_visible(false);
    assert!(doc.refresh_native_signals());
    assert!(texts(&doc.root).contains("now background"), "{}", texts(&doc.root));

    rn::app::set_visible(true);
    assert!(doc.refresh_native_signals());
    assert!(texts(&doc.root).contains("now foreground"), "{}", texts(&doc.root));
}
