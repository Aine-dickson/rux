//! What `catch e` sees (Urbandrop's UD-001): a native error's own `kind` and
//! fields, a `rux::Error` unwrapped, and `throw "text"` as an `Error`.

use rux_runtime::native as rn;
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

fn run(script: &str, view: &str) -> String {
    let src = format!("<template><view><text>{view}</text></view></template>\n<script>\n{script}\n</script>");
    let dir = std::env::temp_dir().join(format!("rux_native_errors_{}_{:x}", std::process::id(), src.len()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("app.rux"), src).unwrap();
    let mut doc = Document::load(dir.join("app.rux")).expect("loads");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while doc.has_waiting_tasks() && !doc.settle_tasks() {
        assert!(std::time::Instant::now() < deadline, "no answer came: {}", shown(&doc));
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let errors: Vec<String> =
        doc.diagnostics().warnings.iter().filter(|w| w.is_error()).map(|w| w.message.clone()).collect();
    assert!(errors.is_empty(), "{errors:?}");
    shown(&doc)
}

fn install() {
    let sync = rn::Call::sync(|_| Err(rn::Error::new("phone_required", "add a phone").with("status", 409i64)));
    let later = rn::Call::future(|_| async {
        Err(rn::Error::new("quote_expired", "gone").with("status", 410i64).with("details", "old fare"))
    });
    rn::Module::new("ud001_api")
        .export(rn::Export::function("check", &[], "int", sync))
        .export(rn::Export::function("quote", &[], "int", later))
        .install();
}

#[test]
fn a_native_errors_own_kind_and_fields_are_read_in_catch() {
    install();
    let out = run(
        "use native::ud001_api;\nlet a = signal(\"\");\nlet b = signal(\"\");\n\
         async fn go() {\n\
           try { ud001_api.check(); } catch e { a = `${e.kind} ${e.status} ${e.message}`; }\n\
           try { await ud001_api.quote(); } catch e { b = `${e.kind} ${e.status} ${e.details}`; }\n\
         }\nmounted { go(); }",
        "[{{ a }}][{{ b }}]",
    );
    assert!(out.contains("[phone_required 409 add a phone][quote_expired 410 old fare]"), "{out}");
}

#[test]
fn a_thrown_string_is_caught_as_an_error() {
    let out = run(
        "let a = signal(\"\");\nmounted { try { throw \"empty title\"; } catch e { a = `${e.kind}: ${e.message}`; } }",
        "[{{ a }}]",
    );
    assert!(out.contains("[error: empty title]"), "{out}");
}
