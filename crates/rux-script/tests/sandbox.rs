//! The sandbox half of the capability model: what a document opened from
//! somewhere that is not the app's own gets (roadmap v0.10 item 2). A file
//! never marks itself trusted; the opener hands the engine a [`Sandbox`], and
//! with none the document is trusted, as it has always been.

use rux_script::{take_warnings, Builder, Engine, Limits, Sandbox};

const SCRIPT: &str = "let level = signal(0);\n";
const SPIN: &str = "let n = 0; while n < 10000 { n += 1; }";

fn engine_with(sandbox: Option<Sandbox>) -> Result<Engine, rux_script::ScriptError> {
    let mut b = Builder::new();
    b.interpreted();
    if let Some(s) = sandbox {
        b.sandbox(s);
    }
    b.build(SCRIPT)
}

fn limits(f: impl FnOnce(&mut Limits)) -> Sandbox {
    let mut s = Sandbox::new();
    f(&mut s.limits);
    s
}

#[test]
fn a_sandbox_stops_a_run_at_its_own_step_budget() {
    let mut e = engine_with(Some(limits(|l| l.steps_per_run = 1000))).expect("builds");
    let _ = take_warnings();
    assert!(!e.run_handler(SPIN), "ten thousand steps do not fit in a thousand");
    let said: Vec<String> = take_warnings().into_iter().map(|w| w.message).collect();
    assert!(said.iter().any(|m| m.contains("without finishing")), "{said:?}");
    // The message names the sandbox's budget, not the default one.
    assert!(said.iter().any(|m| m.contains("1000 steps")), "{said:?}");
    // And the engine is still usable afterwards.
    e.run_handler("level = 5");
    assert_eq!(e.eval_display("level", &[]), "5");
}

#[test]
fn the_same_run_finishes_with_no_sandbox() {
    let mut e = engine_with(None).expect("builds");
    let _ = take_warnings();
    assert!(e.run_handler(SPIN));
    assert!(take_warnings().is_empty());
}

#[test]
fn a_default_sandbox_keeps_the_usual_budget() {
    let mut e = engine_with(Some(Sandbox::new())).expect("builds");
    let _ = take_warnings();
    assert!(e.run_handler(SPIN));
}

#[test]
fn a_document_past_the_size_limit_is_refused_at_load() {
    let big = format!("{SCRIPT}let t = \"{}\";\n", "x".repeat(4096));
    let mut b = Builder::new();
    b.interpreted();
    b.sandbox(limits(|l| l.max_source_bytes = 1024));
    let err = b.build(&big).err().expect("refused");
    assert!(err.message.contains("1024"), "names the limit: {}", err.message);
    // The same text loads with no sandbox.
    let mut b = Builder::new();
    b.interpreted();
    assert!(b.build(&big).is_ok());
}

#[test]
fn a_literal_nested_past_the_limit_is_refused_without_overflowing_the_stack() {
    let deep = format!("{SCRIPT}let a = {}1{};\n", "[".repeat(10_000), "]".repeat(10_000));
    let mut b = Builder::new();
    b.interpreted();
    b.sandbox(limits(|l| l.max_nesting = 64));
    let err = b.build(&deep).err().expect("refused");
    assert!(err.message.contains("64"), "names the limit: {}", err.message);
}

#[test]
fn nesting_within_the_limit_loads() {
    let ok = format!("{SCRIPT}let a = {}1{};\n", "[".repeat(10), "]".repeat(10));
    let mut b = Builder::new();
    b.interpreted();
    b.sandbox(limits(|l| l.max_nesting = 64));
    assert!(b.build(&ok).is_ok());
}

// ----- grants ---------------------------------------------------------------

fn build_with(script: &str, sandbox: Option<Sandbox>) -> Result<Engine, rux_script::ScriptError> {
    let mut b = Builder::new();
    b.interpreted();
    if let Some(s) = sandbox {
        b.sandbox(s);
    }
    b.build(script)
}

const COPY: &str = "let level = signal(0);\nfn copy() {\n    clipboard.writeText(\"x\");\n}\n";

#[test]
fn an_ungranted_module_is_refused_at_load_at_the_line_that_uses_it() {
    let err = build_with(COPY, Some(Sandbox::new())).err().expect("refused");
    assert!(err.message.contains("`clipboard`"), "names the module: {}", err.message);
    assert!(err.message.contains("grant"), "says who can fix it: {}", err.message);
    assert_eq!(err.line, Some(3), "the line of the first use");
}

#[test]
fn a_granted_module_loads() {
    assert!(build_with(COPY, Some(Sandbox::new().grant("clipboard"))).is_ok());
}

#[test]
fn a_grant_is_for_the_module_it_names() {
    let err = build_with(COPY, Some(Sandbox::new().grant("settings"))).err().expect("refused");
    assert!(err.message.contains("`clipboard`"), "{}", err.message);
}

#[test]
fn no_sandbox_means_no_refusal() {
    assert!(build_with(COPY, None).is_ok());
}

#[test]
fn the_file_dialogs_are_granted_as_files() {
    let script = "fn pick() {\n    let f = pickFile({});\n}\n";
    let err = build_with(script, Some(Sandbox::new())).err().expect("refused");
    assert!(err.message.contains("`files`"), "{}", err.message);
    assert!(build_with(script, Some(Sandbox::new().grant("files"))).is_ok());
}

#[test]
fn an_app_module_is_refused_unless_granted_by_its_path() {
    let alias = || rux_script::link::Alias {
        local: "shop".into(),
        target: rux_script::link::Target::Module("native/shop".into()),
        line: 2,
    };
    let script = "let level = signal(0);\n";
    let mut b = Builder::new();
    b.interpreted().aliases(vec![alias()]).sandbox(Sandbox::new());
    let err = b.build(script).err().expect("refused");
    assert!(err.message.contains("`native::shop`"), "{}", err.message);
    assert_eq!(err.line, Some(2));
    // Granted, the refusal is gone (the module itself is not installed here).
    let mut b = Builder::new();
    b.interpreted().aliases(vec![alias()]).sandbox(Sandbox::new().grant("native::shop"));
    let result = b.build(script);
    assert!(!result.err().is_some_and(|e| e.message.contains("grant")), "the grant was ignored");
}

#[test]
fn window_and_app_are_in_scope_only_when_granted() {
    // A trusted document always has them, for markup to read. In a sandbox
    // each is there only when granted.
    let read = |sandbox: Option<Sandbox>| {
        let mut e = build_with(SCRIPT, sandbox).expect("builds");
        let _ = take_warnings();
        e.eval_display("window.maximized", &[]);
        take_warnings().is_empty()
    };
    assert!(read(None));
    assert!(!read(Some(Sandbox::new())));
    assert!(read(Some(Sandbox::new().grant("window"))));
}
