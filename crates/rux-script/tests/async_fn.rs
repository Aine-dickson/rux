//! `async fn` and `await`: step 6 of `docs/11-next.md`.
//!
//! Each test registers host functions under names of its own, since the
//! registry is process-wide and tests run side by side.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use rux_reactive::Value;
use rux_script::{host, Builder, Engine};

fn engine(src: &str) -> Engine {
    Builder::new().build(src).unwrap_or_else(|e| panic!("{src}\n{e:?}"))
}

fn text(e: &Engine, name: &str) -> String {
    e.signal_value(name).map(|v| v.to_display()).unwrap_or_else(|| panic!("no signal {name}"))
}

/// Wait for answers to come for `e`'s tasks, and go on with each.
fn settle(e: &mut Engine) -> Vec<u64> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let ready = e.collect_answers();
        if !ready.is_empty() {
            for id in &ready {
                e.resume_task(*id);
            }
            return ready;
        }
        assert!(Instant::now() < deadline, "no answer came");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// `host::<name>(x)` answers `prefix + x` from another thread.
fn echo_later(name: &str, prefix: &'static str) {
    host::register_async(name, move |args, done| {
        let arg = args.first().map(|v| v.to_display()).unwrap_or_default();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(5));
            done.ok(Value::Text(format!("{prefix}{arg}")));
        });
    });
}

#[test]
fn a_handler_starts_an_async_fn_and_its_writes_render_before_and_after_the_await() {
    echo_later("t1_user", "user ");
    let mut e = engine(
        "let status = signal(\"idle\");\n\
         let name = signal(\"\");\n\
         async fn load(id: int) {\n\
           status = \"loading\";\n\
           let u = await host::t1_user(id);\n\
           name = u;\n\
           status = \"done\";\n\
         }\n",
    );
    let changed = e.run_handler_tracked("load(7)");
    assert!(changed.contains("status"), "{changed:?}");
    assert_eq!(text(&e, "status"), "loading");
    assert_eq!(text(&e, "name"), "");
    let started = e.take_started_tasks();
    assert_eq!(started.len(), 1);
    assert!(e.has_task(started[0]));

    let ready = settle(&mut e);
    assert_eq!(ready, started);
    assert_eq!(text(&e, "name"), "user 7");
    assert_eq!(text(&e, "status"), "done");
    assert!(!e.has_task(started[0]), "finished");
    assert!(e.take_task_failures().is_empty());
}

#[test]
fn resuming_reports_what_the_task_changed() {
    echo_later("t2_x", "");
    let mut e = engine("let a = signal(0);\nlet b = signal(\"\");\nasync fn go() { let v = await host::t2_x(1); b = v; }\n");
    e.run_handler("go()");
    let id = e.take_started_tasks()[0];
    let deadline = Instant::now() + Duration::from_secs(5);
    while e.collect_answers().is_empty() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let changed = e.resume_task(id);
    assert!(changed.contains("b") && !changed.contains("a"), "{changed:?}");
}

#[test]
fn an_async_fn_awaits_another_and_gets_its_result() {
    echo_later("t3_part", "p");
    let mut e = engine(
        "let out = signal(\"\");\n\
         async fn part(n: int): string { let s = await host::t3_part(n); return s + \"!\"; }\n\
         async fn whole() { let a = await part(1); let b = await part(2); out = a + b; }\n",
    );
    e.run_handler("whole()");
    assert_eq!(e.take_started_tasks().len(), 1, "one task, however many functions it awaits");
    settle(&mut e);
    assert_eq!(text(&e, "out"), "");
    settle(&mut e);
    assert_eq!(text(&e, "out"), "p1!p2!");
}

#[test]
fn a_failed_await_throws_where_it_is_and_catch_takes_it() {
    host::register_async("t4_bad", |_, done| done.fail("no network"));
    let mut e = engine(
        "let status = signal(\"\");\n\
         async fn save() {\n\
           try {\n\
             await host::t4_bad();\n\
             status = \"saved\";\n\
           } catch e {\n\
             status = \"could not save: \" + e.message;\n\
           }\n\
         }\n",
    );
    e.run_handler("save()");
    settle(&mut e);
    assert_eq!(text(&e, "status"), "could not save: no network");
    assert!(e.take_task_failures().is_empty());
}

#[test]
fn an_error_nothing_catches_is_the_tasks_and_names_its_line() {
    host::register_async("t5_bad", |_, done| done.fail("gone"));
    let mut e = engine(
        "let n = signal(0);\n\
         async fn go() {\n\
           n = 1;\n\
           await host::t5_bad();\n\
           n = 2;\n\
         }\n",
    );
    assert!(e.run_handler("go()"), "the handler itself did not fail");
    settle(&mut e);
    assert_eq!(text(&e, "n"), "1", "the write after the failed await never ran");
    let failures = e.take_task_failures();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].message.contains("async fn go") && failures[0].message.contains("gone"), "{failures:?}");
    assert_eq!(failures[0].line, Some(4));
}

#[test]
fn awaits_inside_loops_conditions_and_expressions() {
    echo_later("t6_n", "");
    let mut e = engine(
        "let log = signal(\"\");\n\
         async fn num(n: int): int { let s = await host::t6_n(n); return parseInt(s) ?? 0; }\n\
         async fn go() {\n\
           let total = 0;\n\
           for i in 0..3 { total += await num(i); }\n\
           let items = [10, 20];\n\
           for x in items { if x > 10 { total += await num(x); } }\n\
           let k = 0;\n\
           while await num(k) < 2 { k += 1; }\n\
           let pick = if total > 0 { await num(5) } else { 0 };\n\
           let both = total > 0 && await num(1) == 1;\n\
           let skipped = total < 0 && await num(99) == 99;\n\
           let label = switch k { 2 => `two ${await num(2)}`, _ => \"other\" };\n\
           log = `${total} ${k} ${pick} ${both} ${skipped} ${label}`;\n\
         }\n",
    );
    e.run_handler("go()");
    let deadline = Instant::now() + Duration::from_secs(20);
    while e.signal_value("log").map(|v| v.to_display()).unwrap_or_default().is_empty() {
        assert!(Instant::now() < deadline, "never finished");
        settle(&mut e);
    }
    assert_eq!(text(&e, "log"), "23 2 5 true false two 2");
    assert!(e.take_task_failures().is_empty());
}

#[test]
fn what_an_expression_read_before_an_await_is_what_it_uses() {
    echo_later("t7_n", "");
    let mut e = engine(
        "let n = signal(1);\n\
         let out = signal(0);\n\
         async fn go() { out = n + parseInt(await host::t7_n(10)) ?? 0; }\n",
    );
    e.run_handler("go()");
    // `n` changes while `go` waits; `go` read it before.
    e.run_handler("n = 100");
    settle(&mut e);
    assert_eq!(text(&e, "out"), "11");
}

#[test]
fn a_mutating_method_with_an_awaited_argument_changes_its_receiver() {
    echo_later("t8_item", "item ");
    let mut e = engine("let items = signal([]);\nasync fn add() { items.push(await host::t8_item(1)); }\n");
    e.run_handler("add()");
    settle(&mut e);
    assert_eq!(e.eval_display("`${items.length} ${items[0]}`", &[]), "1 item 1");
}

#[test]
fn a_dropped_task_never_goes_on() {
    let (tx, rx) = mpsc::channel::<host::Completer>();
    let tx = std::sync::Mutex::new(tx);
    host::register_async("t9_slow", move |_, done| {
        tx.lock().unwrap().send(done).unwrap();
    });
    let mut e = engine("let n = signal(0);\nasync fn go() { await host::t9_slow(); n = 1; }\n");
    e.run_handler("go()");
    let id = e.take_started_tasks()[0];
    e.drop_tasks(&[id]);
    rx.recv().unwrap().ok(Value::Null);
    assert!(e.collect_answers().is_empty());
    assert_eq!(text(&e, "n"), "0");
    assert!(!e.has_task(id));
}

#[test]
fn an_answer_given_at_once_still_waits_for_the_runtime() {
    host::register_async("t10_now", |_, done| done.ok(Value::Number(3.0)));
    let mut e = engine("let n = signal(0);\nasync fn go() { n = await host::t10_now(); }\n");
    e.run_handler("go()");
    assert_eq!(text(&e, "n"), "0", "everything between two stops runs without interruption");
    settle(&mut e);
    assert_eq!(text(&e, "n"), "3");
}

#[test]
fn an_async_fn_that_never_waits_finishes_in_the_handler() {
    let mut e = engine("let n = signal(0);\nasync fn go() { n = 5; }\n");
    e.run_handler("go()");
    assert_eq!(text(&e, "n"), "5");
    assert!(e.take_started_tasks().is_empty());
}

#[test]
fn a_host_answer_that_never_comes_is_an_error_not_a_hang() {
    host::register_async("t11_forgot", |_, done| drop(done));
    let mut e = engine("let s = signal(\"\");\nasync fn go() { try { await host::t11_forgot(); } catch e { s = e.message; } }\n");
    e.run_handler("go()");
    settle(&mut e);
    assert!(text(&e, "s").contains("finished without answering"), "{}", text(&e, "s"));
}

#[test]
fn an_effect_is_not_subscribed_to_what_its_task_reads() {
    host::register_async("t12_wait", |_, done| done.ok(Value::Null));
    let mut e = engine(
        "let id = signal(1);\n\
         let other = signal(0);\n\
         let seen = signal(0);\n\
         async fn load(n: int) { seen = other; await host::t12_wait(); seen = other + n; }\n",
    );
    let (reads, _) = e.run_effect_tracked("load(id)");
    assert!(reads.contains("id"), "{reads:?}");
    assert!(!reads.contains("other"), "{reads:?}");
}
