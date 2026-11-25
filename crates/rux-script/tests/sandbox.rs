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

// ----- memory ---------------------------------------------------------------

/// Run `handler` under a sandbox, and say what it was stopped with, if it was.
fn stopped_with(sandbox: Sandbox, handler: &str) -> Option<String> {
    let mut e = build_with(SCRIPT, Some(sandbox)).expect("builds");
    let _ = take_warnings();
    let ok = e.run_handler(handler);
    let said: Vec<String> = take_warnings().into_iter().map(|w| w.message).collect();
    if ok {
        assert!(said.is_empty(), "{said:?}");
        None
    } else {
        Some(said.join("; "))
    }
}

#[test]
fn a_huge_repeat_is_refused_before_it_allocates() {
    let started = std::time::Instant::now();
    let said = stopped_with(Sandbox::new(), "let s = \"x\".repeat(100000000);").expect("stopped");
    assert!(said.contains("sandboxed"), "{said}");
    assert!(started.elapsed() < std::time::Duration::from_secs(1), "it built the text first");
}

#[test]
fn a_string_that_doubles_is_stopped_at_the_text_limit() {
    let said = stopped_with(limits(|l| l.max_text_bytes = 1 << 20), "let s = \"x\"; while true { s = s + s; }")
        .expect("stopped");
    assert!(said.contains("longer than"), "{said}");
}

#[test]
fn a_template_past_the_text_limit_is_stopped() {
    let said = stopped_with(
        limits(|l| l.max_text_bytes = 100),
        "let s = \"x\".repeat(60); let t = `${s}${s}`;",
    )
    .expect("stopped");
    assert!(said.contains("longer than"), "{said}");
}

#[test]
fn padding_past_the_text_limit_is_stopped() {
    let said = stopped_with(limits(|l| l.max_text_bytes = 1000), "let s = \"a\".padStart(100000, \"-\");").expect("stopped");
    assert!(said.contains("longer than"), "{said}");
}

#[test]
fn pushing_past_the_item_limit_is_stopped() {
    let said = stopped_with(
        limits(|l| l.max_items = 100),
        "let a = [0]; let i = 0; while i < 1000 { a.push(i); i += 1; }",
    )
    .expect("stopped");
    assert!(said.contains("entries"), "{said}");
}

#[test]
fn adding_to_a_list_with_plus_equals_is_counted_too() {
    let said = stopped_with(
        limits(|l| l.max_items = 100),
        "let a = [0]; let i = 0; while i < 1000 { a += i; i += 1; }",
    )
    .expect("stopped");
    assert!(said.contains("entries"), "{said}");
}

#[test]
fn a_map_that_grows_past_the_item_limit_is_stopped() {
    let said = stopped_with(
        limits(|l| l.max_items = 100),
        "let m = {}; let i = 0; while i < 1000 { m.set(`k${i}`, i); i += 1; }",
    )
    .expect("stopped");
    assert!(said.contains("entries"), "{said}");
}

#[test]
fn what_one_run_builds_is_capped_even_when_no_one_piece_is_too_big() {
    let said = stopped_with(
        limits(|l| l.run_alloc_bytes = 10_000),
        "let all = []; let i = 0; while i < 1000 { all.push([1, 2, 3]); i += 1; }",
    )
    .expect("stopped");
    assert!(said.contains("in one run"), "{said}");
}

#[test]
fn json_nested_past_the_limit_fails_the_parse() {
    let deep = format!("{}1{}", "[".repeat(20), "]".repeat(20));
    let handler = format!("let v = JSON.parse(\"{deep}\");");
    let said = stopped_with(limits(|l| l.max_nesting = 8), &handler).expect("stopped");
    assert!(said.contains("nested more than 8"), "{said}");
    assert!(stopped_with(limits(|l| l.max_nesting = 64), &handler).is_none());
}

#[test]
fn json_with_more_entries_than_the_limit_fails_the_parse() {
    let said = stopped_with(
        limits(|l| l.max_items = 5),
        "let v = JSON.parse(\"[1,2,3,4,5,6,7,8,9]\");",
    )
    .expect("stopped");
    assert!(said.contains("entries"), "{said}");
}

#[test]
fn ordinary_work_is_not_counted_as_too_much() {
    // A long string built a character at a time is the usual shape of a
    // legitimate loop: what it copies is not what it holds.
    assert!(stopped_with(
        Sandbox::new(),
        "let s = \"\"; let i = 0; while i < 20000 { s += \"x\"; i += 1; }"
    )
    .is_none());
    assert!(stopped_with(Sandbox::new(), "let a = []; let i = 0; while i < 20000 { a.push(i); i += 1; }").is_none());
}

#[test]
fn a_trusted_document_has_no_such_limits() {
    let mut e = build_with(SCRIPT, None).expect("builds");
    let _ = take_warnings();
    assert!(e.run_handler("let s = \"x\".repeat(40000000);"));
    assert!(take_warnings().is_empty());
}

#[test]
fn a_trusted_json_parse_still_has_a_depth_it_stops_at() {
    let deep = |n: usize| format!("let v = JSON.parse(\"{}1{}\");", "[".repeat(n), "]".repeat(n));
    let mut e = build_with(SCRIPT, None).expect("builds");
    let _ = take_warnings();
    assert!(e.run_handler(&deep(300)));
    assert!(take_warnings().is_empty());
    assert!(!e.run_handler(&deep(100_000)));
    let said: Vec<String> = take_warnings().into_iter().map(|w| w.message).collect();
    assert!(said.join("; ").contains("nested more than 512 deep"), "{said:?}");
}

// ----- state across runs -------------------------------------------------------

const KEEPS: &str = "let log = [];\n";
const KEEP_40K: &str = "log.push(\"x\".repeat(40000))";

#[test]
fn a_run_that_would_take_the_state_past_its_limit_is_undone_and_reported() {
    let mut e = build_with(KEEPS, Some(limits(|l| l.state_bytes = 100_000))).expect("builds");
    let _ = take_warnings();
    for _ in 0..2 {
        assert!(!e.run_handler_tracked(KEEP_40K).is_empty());
    }
    assert!(take_warnings().is_empty());
    // The third would hold 120,000 bytes.
    let changed = e.run_handler_tracked(KEEP_40K);
    let said: Vec<String> = take_warnings().into_iter().map(|w| w.message).collect();
    assert!(said.iter().any(|m| m.contains("state") && m.contains("100000")), "{said:?}");
    assert!(changed.is_empty(), "nothing changed, since it was undone: {changed:?}");
    assert_eq!(e.eval_display("log.length", &[]), "2");
    // Taking something out makes room again.
    e.run_handler_tracked("log.pop()");
    assert!(!e.run_handler_tracked(KEEP_40K).is_empty());
    assert!(take_warnings().is_empty());
}

#[test]
fn a_shared_value_is_counted_once() {
    // `a = [a, a]` thirty times is a billion entries to walk, and a few
    // hundred bytes to hold.
    let mut e = build_with("let a = [];\n", Some(limits(|l| l.state_bytes = 100_000))).expect("builds");
    let _ = take_warnings();
    let started = std::time::Instant::now();
    assert!(!e.run_handler_tracked("let i = 0; while i < 30 { a = [a, a]; i += 1; }").is_empty());
    assert!(take_warnings().is_empty());
    assert!(started.elapsed() < std::time::Duration::from_secs(2));
}

#[test]
fn a_trusted_document_keeps_what_it_likes() {
    let mut e = build_with(KEEPS, None).expect("builds");
    let _ = take_warnings();
    for _ in 0..5 {
        assert!(!e.run_handler_tracked(KEEP_40K).is_empty());
    }
    assert!(take_warnings().is_empty());
}
