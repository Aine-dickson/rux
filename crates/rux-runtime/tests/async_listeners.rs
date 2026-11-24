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

/// Urbandrop's UD-012 as the client app has it, on the desktop: the page
/// shows `<sign-in />` under `r-elif="!signedIn"`, so the signal the
/// component's await sets removes that component while its task waits. The
/// page's `effect` then starts `load()`, which awaits two native calls; the
/// app's `effect` hands the session to a store.
#[test]
fn a_component_removed_by_the_signal_its_await_sets_lets_the_pages_load_finish() {
    static SIGNED: rn::Signal<bool> = rn::Signal::new();
    let verify = rn::Call::future(|_| async {
        std::thread::sleep(std::time::Duration::from_millis(5));
        SIGNED.set(true);
        rn::notify();
        std::thread::sleep(std::time::Duration::from_millis(20));
        Ok(rn::Any::Str("ok".into()))
    });
    let orders = rn::Call::future(|_| async {
        std::thread::sleep(std::time::Duration::from_millis(10));
        Ok(rn::Any::Str("3 orders".into()))
    });
    let money = rn::Call::future(|_| async {
        std::thread::sleep(std::time::Duration::from_millis(10));
        Ok(rn::Any::Str("2 payments".into()))
    });
    rn::Module::new("ud012_client")
        .export(rn::Export::signal("signedIn", "bool?", || SIGNED.__read()))
        .export(rn::Export::function("verify", &[], "string", verify))
        .export(rn::Export::function("myOrders", &[], "string", orders))
        .export(rn::Export::function("transactions", &[], "string", money))
        .install();
    let sign_in = "<template><view><text>[{{ if busy { \"Checking\" } else { \"Verify\" } }}]</text></view></template>\n\
<script>\nuse native::ud012_client;\nlet busy = signal(false);\nlet error = signal(\"\");\n\
async fn verify() { busy = true; error = \"\"; try { await ud012_client.verify(); } catch e { error = e.message; } busy = false; }\n\
mounted { verify(); }\n</script>";
    let page = "<template><view>\
<text r-if=\"!session.ready\">Loading...</text>\
<view r-elif=\"!signedIn\"><text>Sign in</text><sign-in /></view>\
<view r-else><text>[{{ list }}][{{ pays }}][{{ loading }}][{{ error }}]</text></view>\
</view></template>\n\
<script>\nuse native::ud012_client;\nuse stores::session;\nuse components::sign_in;\n\
let list = signal(\"\");\nlet pays = signal(\"\");\nlet loading = signal(false);\nlet error = signal(\"\");\n\
computed signedIn = ud012_client.signedIn ?? false;\n\
async fn load() { if !signedIn { return; } loading = true; error = \"\"; \
try { list = await ud012_client.myOrders(); pays = await ud012_client.transactions(); } catch e { error = e.message; } loading = false; }\n\
effect { if signedIn { load(); } }\n</script>";
    let store = "<script>\nexport let ready = signal(false);\nexport let kept = signal(0);\n\
export async fn start() { ready = true; }\n\
export fn keep(s: bool?) { if s == none { return; } kept = kept + 1; }\n</script>";
    let app = "<template><screen><router><route path=\"/\" view=\"account\" /></router></screen></template>\n\
<script>\nuse native::ud012_client;\nuse pages::account;\nuse stores::session;\n\
effect { session.keep(ud012_client.signedIn); }\nmounted { session.start(); }\n</script>";
    let dir = std::env::temp_dir().join(format!("rux_ud012c_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (rel, body) in [
        ("app.rux", app),
        ("pages/account.rux", page),
        ("components/sign_in.rux", sign_in),
        ("stores/session.rux", store),
    ] {
        std::fs::create_dir_all(dir.join(rel).parent().unwrap()).unwrap();
        std::fs::write(dir.join(rel), body).unwrap();
    }
    let mut doc = Document::load(dir.join("app.rux")).expect("loads");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut seen_checking = false;
    loop {
        seen_checking |= shown(&doc).contains("[Checking]");
        let _ = doc.refresh_native_signals();
        let _ = doc.settle_tasks();
        if !doc.has_waiting_tasks() && shown(&doc).contains("[false]") {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "still waiting: {}", shown(&doc));
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let errors: Vec<String> =
        doc.diagnostics().warnings.iter().filter(|w| w.is_error()).map(|w| w.message.clone()).collect();
    assert!(errors.is_empty(), "{errors:?}");
    assert!(seen_checking, "the sign-in never started: {}", shown(&doc));
    assert!(shown(&doc).contains("[3 orders][2 payments][false][]"), "{}", shown(&doc));
}

/// Urbandrop's UD-012 on Android, as the booking's You step has it: the page
/// shows `<sign-in />` under `r-else` of `r-if="signedIn"`, and the component
/// binds inputs with `r-model`. Its `verify()` awaits a native call that sets
/// the session first, so the component is gone when the task goes on to write
/// `busy = false`. Nothing of the removed component may be evaluated again,
/// and the page must show the signed-in branch. `gap_ms` is the time between
/// the signal and the answer: on the emulator both often land in one wake.
fn removed_component_with_inputs(module: &'static str, signed: &'static rn::Signal<bool>, gap_ms: u64) {
    let verify = rn::Call::future(move |_| async move {
        std::thread::sleep(std::time::Duration::from_millis(5));
        signed.set(true);
        rn::notify();
        if gap_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(gap_ms));
        }
        Ok(rn::Any::Str("ok".into()))
    });
    rn::Module::new(module)
        .export(rn::Export::signal("signedIn", "bool?", move || signed.__read()))
        .export(rn::Export::function("verify", &[], "string", verify))
        .install();
    let sign_in = format!(
        "<template><view><input r-model=\"email\" /><input r-model=\"code\" /><input r-model=\"phone\" />\
<text>[{{{{ if busy {{ \"Checking\" }} else {{ \"Verify\" }} }}}}][{{{{ error }}}}]</text></view></template>\n\
<script>\nuse native::{module};\nlet email = signal(\"a@b.c\");\nlet code = signal(\"123456\");\nlet phone = signal(\"\");\n\
let busy = signal(false);\nlet error = signal(\"\");\n\
async fn verify() {{ busy = true; try {{ await {module}.verify(); }} catch e {{ error = e.message; }} busy = false; }}\n\
mounted {{ verify(); }}\n</script>"
    );
    let page = format!(
        "<template><view><text>You</text>\
<view r-if=\"signedIn\"><text>Signed in as someone</text></view>\
<view r-else><sign-in /></view></view></template>\n\
<script>\nuse native::{module};\nuse components::sign_in;\ncomputed signedIn = {module}.signedIn ?? false;\n</script>"
    );
    let app = "<template><screen><router><route path=\"/\" view=\"book\" /></router></screen></template>\n\
<script>\nuse pages::book;\n</script>";
    let dir = std::env::temp_dir().join(format!("rux_{module}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (rel, body) in [("app.rux", app), ("pages/book.rux", page.as_str()), ("components/sign_in.rux", sign_in.as_str())] {
        std::fs::create_dir_all(dir.join(rel).parent().unwrap()).unwrap();
        std::fs::write(dir.join(rel), body).unwrap();
    }
    let mut doc = Document::load(dir.join("app.rux")).expect("loads");
    assert!(shown(&doc).contains("[Checking]"), "{}", shown(&doc));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let _ = doc.refresh_native_signals();
        let _ = doc.settle_tasks();
        if !doc.has_waiting_tasks() {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "still waiting: {}", shown(&doc));
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let errors: Vec<String> =
        doc.diagnostics().warnings.iter().filter(|w| w.is_error()).map(|w| w.message.clone()).collect();
    assert!(errors.is_empty(), "{errors:?}");
    assert!(shown(&doc).contains("Signed in as someone"), "{}", shown(&doc));
    assert!(!shown(&doc).contains("Checking"), "{}", shown(&doc));
}

#[test]
fn a_removed_component_with_inputs_is_not_evaluated_again_when_its_task_goes_on() {
    static SIGNED: rn::Signal<bool> = rn::Signal::new();
    removed_component_with_inputs("ud012_you_gap", &SIGNED, 20);
}

#[test]
fn a_removed_component_with_inputs_is_not_evaluated_again_when_signal_and_answer_land_together() {
    static SIGNED: rn::Signal<bool> = rn::Signal::new();
    removed_component_with_inputs("ud012_you_together", &SIGNED, 0);
}

/// UD-012 as the booking's You step has it in full: the component sends a
/// code (its "Send code" branch goes), then verifies; its `:disabled`
/// bindings read `busy` first and call native functions; the page is on step
/// 4 of an `r-if`/`r-elif` chain and its `effect` copies the session's name
/// into a page signal the moment it arrives.
fn you_step_in_full(module: &'static str, signed: &'static rn::Signal<bool>, gap_ms: u64, settle_first: bool) {
    let send = rn::Call::future(|_| async {
        std::thread::sleep(std::time::Duration::from_millis(3));
        Ok(rn::Any::None)
    });
    let verify = rn::Call::future(move |_| async move {
        std::thread::sleep(std::time::Duration::from_millis(5));
        signed.set(true);
        rn::notify();
        if gap_ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(gap_ms));
        }
        Ok(rn::Any::None)
    });
    let is_email = rn::Call::sync(|args| Ok(rn::Any::Bool(matches!(args.first(), Some(rn::Any::Str(s)) if s.contains('@')))));
    rn::Module::new(module)
        .export(rn::Export::signal("signedIn", "bool?", move || signed.__read()))
        .export(rn::Export::function("requestCode", &[], "void", send))
        .export(rn::Export::function("verifyCode", &[], "void", verify))
        .export(rn::Export::function("isEmail", &[("s", "string")], "bool", is_email))
        .install();
    let sign_in = format!(
        "<template><view><input r-model=\"email\" />\
<view r-if=\"!codeSent\"><button :disabled=\"busy || !{module}.isEmail(email)\"><text>{{{{ if busy {{ \"Sending...\" }} else {{ \"Send code\" }} }}}}</text></button></view>\
<view r-else><input r-model=\"code\" /><input r-model=\"phone\" />\
<button :disabled=\"busy || code.trim().length != 6 || phone.length > 9\"><text>{{{{ if busy {{ \"Checking...\" }} else {{ \"Verify\" }} }}}}</text></button></view>\
<text r-if='error != \"\"'>{{{{ error }}}}</text></view></template>\n\
<script>\nuse native::{module};\nlet email = signal(\"a@b.c\");\nlet code = signal(\"123456\");\nlet phone = signal(\"\");\n\
let codeSent = signal(false);\nlet busy = signal(false);\nlet error = signal(\"\");\n\
async fn sendCode() {{ busy = true; error = \"\"; try {{ await {module}.requestCode(); codeSent = true; }} catch e {{ error = e.message; }} busy = false; }}\n\
async fn verify() {{ busy = true; error = \"\"; try {{ await {module}.verifyCode(); }} catch e {{ error = e.message; }} busy = false; }}\n\
mounted {{ go(); }}\nasync fn go() {{ await sendCode(); await verify(); }}\n</script>"
    );
    let page = format!(
        "<template><view>\
<view r-if=\"step == 1\"><text>Parcel</text></view>\
<view r-elif=\"step == 4\"><text>Your contact details</text>\
<view r-if=\"signedIn\"><text>Signed in as [{{{{ name }}}}]</text><input r-model=\"name\" /></view>\
<view r-else><sign-in /></view></view>\
<view r-elif=\"step == 5\"><text>Confirm</text></view>\
<button :disabled=\"!canNext\"><text>Continue</text></button></view></template>\n\
<script>\nuse native::{module};\nuse components::sign_in;\nlet step = signal(4);\nlet name = signal(\"\");\n\
computed signedIn = {module}.signedIn ?? false;\ncomputed canNext = if step == 4 {{ signedIn && name.trim().length >= 3 }} else {{ true }};\n\
effect {{ if signedIn {{ if name == \"\" {{ name = \"Amina\"; }} }} }}\n</script>"
    );
    let app = "<template><screen><router><route path=\"/\" view=\"book\" /></router></screen></template>\n\
<script>\nuse pages::book;\n</script>";
    let dir = std::env::temp_dir().join(format!("rux_{module}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (rel, body) in [("app.rux", app), ("pages/book.rux", page.as_str()), ("components/sign_in.rux", sign_in.as_str())] {
        std::fs::create_dir_all(dir.join(rel).parent().unwrap()).unwrap();
        std::fs::write(dir.join(rel), body).unwrap();
    }
    let mut doc = Document::load(dir.join("app.rux")).expect("loads");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut seen_checking = false;
    loop {
        seen_checking |= shown(&doc).contains("Checking...");
        // The shell settles answers when they come and reads native signals
        // when it draws, so an answer and a signal in one wake are settled first.
        if settle_first {
            let _ = doc.settle_tasks();
            let _ = doc.refresh_native_signals();
        } else {
            let _ = doc.refresh_native_signals();
            let _ = doc.settle_tasks();
        }
        if !doc.has_waiting_tasks() {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "still waiting: {}", shown(&doc));
        // Long enough, settling first, that the session and the answer have
        // both landed by the next round, as in one wake on the emulator.
        std::thread::sleep(std::time::Duration::from_millis(if settle_first { 30 } else { 1 }));
    }
    let errors: Vec<String> =
        doc.diagnostics().warnings.iter().filter(|w| w.is_error()).map(|w| w.message.clone()).collect();
    assert!(seen_checking, "never verified: {}", shown(&doc));
    assert!(errors.is_empty(), "{errors:?}");
    assert!(shown(&doc).contains("Signed in as [Amina]"), "{}", shown(&doc));
    assert!(!shown(&doc).contains("Checking"), "{}", shown(&doc));
}

#[test]
fn the_you_step_signs_in_with_a_gap_between_the_session_and_the_answer() {
    static SIGNED: rn::Signal<bool> = rn::Signal::new();
    you_step_in_full("ud012_full_gap", &SIGNED, 20, false);
}

#[test]
fn the_you_step_signs_in_when_the_session_and_the_answer_land_together() {
    static SIGNED: rn::Signal<bool> = rn::Signal::new();
    you_step_in_full("ud012_full_together", &SIGNED, 0, false);
}

#[test]
fn the_you_step_signs_in_when_the_answer_is_settled_before_the_session_is_read() {
    static SIGNED: rn::Signal<bool> = rn::Signal::new();
    you_step_in_full("ud012_full_settled", &SIGNED, 0, true);
}
