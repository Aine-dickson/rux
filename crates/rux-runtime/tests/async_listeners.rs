//! An `async fn` a listener starts is the caller's: after a native `await`
//! it goes on in the caller's scope, not the emitting component's
//! (Urbandrop's UD-010).

use rux_runtime::native as rn;
use rux_runtime::Document;

fn project(files: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rux_async_listeners_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (rel, body) in files {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    dir
}

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
fn a_listeners_async_fn_goes_on_in_the_callers_scope_after_a_native_await() {
    let call = rn::Call::future(|_| async {
        std::thread::sleep(std::time::Duration::from_millis(10));
        Ok(rn::Any::Str("ok".into()))
    });
    rn::Module::new("ud010_save").export(rn::Export::function("save", &[], "string", call)).install();
    // The form has a prop `busy` and no `error`, as Urbandrop's does.
    let form = "<template><view><text>form</text></view></template>\n\
<script>\n  prop busy = false;\n  mounted { emit(\"submit\", 1); }\n</script>";
    let page = "<template><view><form-fields :busy=\"busy\" @submit=\"save(event)\" />\
<text>[{{ error }}][{{ note }}][{{ busy }}]</text></view></template>\n\
<script>\nuse native::ud010_save;\nuse components::form_fields;\n\
let busy = signal(false);\nlet error = signal(\"\");\nlet note = signal(\"\");\n\
fn saved(m: string) { note = m; }\n\
async fn save(n: int) { busy = true; error = \"\"; try { let r = await ud010_save.save(); saved(r); } catch e { error = e.message; } busy = false; }\n\
</script>";
    let app = "<template><screen><router><route path=\"/\" view=\"page\" /></router></screen></template>\n\
<script>\nuse pages::page;\n</script>";
    let dir = project(&[("app.rux", app), ("pages/page.rux", page), ("components/form_fields.rux", form)]);
    let mut doc = Document::load(dir.join("app.rux")).expect("loads");
    assert!(doc.has_waiting_tasks(), "the save is waiting: {}", shown(&doc));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !doc.settle_tasks() {
        assert!(std::time::Instant::now() < deadline, "no answer came");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let errors: Vec<String> =
        doc.diagnostics().warnings.iter().filter(|w| w.is_error()).map(|w| w.message.clone()).collect();
    assert!(errors.is_empty(), "{errors:?}");
    assert!(shown(&doc).contains("[][ok][false]"), "{}", shown(&doc));
}

/// Urbandrop's UD-012: an `async fn` an instance's `effect` starts as the
/// instance is made, on a page with no `mounted`, was owned by nobody, so its
/// answer was taken and nothing after the `await` ever ran.
#[test]
fn an_effects_async_fn_started_as_the_page_is_made_goes_on_after_its_await() {
    let call = rn::Call::future(|_| async {
        std::thread::sleep(std::time::Duration::from_millis(10));
        Ok(rn::Any::Str("3 orders".into()))
    });
    rn::Module::new("ud012_orders").export(rn::Export::function("mine", &[], "string", call)).install();
    let page = "<template><view><text>[{{ if loading { \"Loading\" } else { list } }}]</text></view></template>\n\
<script>\nuse native::ud012_orders;\nlet signedIn = signal(true);\nlet loading = signal(false);\nlet list = signal(\"\");\n\
async fn load() { loading = true; try { list = await ud012_orders.mine(); } catch e { list = e.message; } loading = false; }\n\
effect { if signedIn { load(); } }\n</script>";
    let app = "<template><screen><router><route path=\"/\" view=\"account\" /></router></screen></template>\n\
<script>\nuse pages::account;\n</script>";
    let dir = std::env::temp_dir().join(format!("rux_ud012_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("pages")).unwrap();
    std::fs::write(dir.join("app.rux"), app).unwrap();
    std::fs::write(dir.join("pages/account.rux"), page).unwrap();
    let mut doc = Document::load(dir.join("app.rux")).expect("loads");
    assert!(shown(&doc).contains("[Loading]"), "{}", shown(&doc));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !doc.settle_tasks() {
        assert!(std::time::Instant::now() < deadline, "the answer was never settled: {}", shown(&doc));
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let errors: Vec<String> =
        doc.diagnostics().warnings.iter().filter(|w| w.is_error()).map(|w| w.message.clone()).collect();
    assert!(errors.is_empty(), "{errors:?}");
    assert!(shown(&doc).contains("[3 orders]"), "{}", shown(&doc));
}

/// Urbandrop's UD-012, on Android: a component's `async fn` awaits a native
/// call that sets a native signal the page reads (`api.session`) before it
/// answers. The page has to follow the signal, and the component's task has
/// to go on after its `await`.
#[test]
fn a_native_signal_set_during_a_components_await_reaches_the_page_and_the_task_goes_on() {
    static SESSION: rn::Signal<bool> = rn::Signal::new();
    let call = rn::Call::future(|_| async {
        std::thread::sleep(std::time::Duration::from_millis(5));
        SESSION.set(true);
        rn::notify();
        std::thread::sleep(std::time::Duration::from_millis(5));
        Ok(rn::Any::Str("ok".into()))
    });
    rn::Module::new("ud012_api")
        .export(rn::Export::signal("signedIn", "bool?", || SESSION.__read()))
        .export(rn::Export::function("verify", &[], "string", call))
        .install();
    let sign_in = "<template><view><text>[{{ if busy { \"Checking\" } else { said } }}]</text></view></template>\n\
<script>\nuse native::ud012_api;\nlet busy = signal(false);\nlet said = signal(\"\");\nlet error = signal(\"\");\n\
async fn verify() { busy = true; try { said = await ud012_api.verify(); } catch e { error = e.message; } busy = false; }\n\
mounted { verify(); }\n</script>";
    let page = "<template><view><text r-if=\"ud012_api.signedIn ?? false\">signed in</text><sign-in /></view></template>\n\
<script>\nuse native::ud012_api;\nuse components::sign_in;\n</script>";
    let app = "<template><screen><router><route path=\"/\" view=\"book\" /></router></screen></template>\n\
<script>\nuse pages::book;\n</script>";
    let dir = std::env::temp_dir().join(format!("rux_ud012b_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("pages")).unwrap();
    std::fs::create_dir_all(dir.join("components")).unwrap();
    std::fs::write(dir.join("app.rux"), app).unwrap();
    std::fs::write(dir.join("pages/book.rux"), page).unwrap();
    std::fs::write(dir.join("components/sign_in.rux"), sign_in).unwrap();
    let mut doc = Document::load(dir.join("app.rux")).expect("loads");
    assert!(shown(&doc).contains("[Checking]"), "{}", shown(&doc));
    // As the shell does: a notify wakes it to refresh native signals, an
    // answer wakes it to settle tasks.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let _ = doc.refresh_native_signals();
        let _ = doc.settle_tasks();
        if !doc.has_waiting_tasks() {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "still waiting: {}", shown(&doc));
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let errors: Vec<String> =
        doc.diagnostics().warnings.iter().filter(|w| w.is_error()).map(|w| w.message.clone()).collect();
    assert!(errors.is_empty(), "{errors:?}");
    assert!(shown(&doc).contains("signed in"), "{}", shown(&doc));
    assert!(shown(&doc).contains("[ok]"), "{}", shown(&doc));
}
