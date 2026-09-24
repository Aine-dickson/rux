//! A name read inside a `fn` body that nothing anywhere declares.
//!
//! `fn addUser() { Have }` passed `rux check` clean, in a page as well as in a
//! skipped component: the undefined-name check runs when an expression is
//! *evaluated*, and a `fn` nobody has called yet is never evaluated. Found in
//! the user's own file, 2026-09-17, while chasing something else.
//!
//! It is an **error**: a name declared nowhere cannot resolve under any caller.
//!
//! **The check cannot be the obvious one**, and these tests exist as much to
//! pin that as to pin the catch. Divergence 4 in the rhai fork makes a call run
//! in the scope it was written in, so a `fn` sees its caller's locals; checking
//! a body against its own parameters would report the single most useful thing
//! the fork exists to allow. So the question asked is the weaker one: is this
//! name declared *anywhere*?

use rux_runtime::Document;

/// The messages this document produced, joined so a test can look for one.
fn problems(src: &str) -> String {
    let doc = Document::from_source(src).expect("loads");
    doc.diagnostics()
        .warnings
        .iter()
        .map(|w| w.message.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The reported case, reduced.
#[test]
fn a_name_declared_nowhere_is_reported() {
    let found = problems(
        "<template><screen><text>hi</text></screen></template>\
         <script>let n = signal(0);\nfn add() {\nHave\n}</script>",
    );
    assert!(found.contains("`Have`"), "the stray name is not reported: {found}");
    assert!(found.contains("declared nowhere"), "and it must say why: {found}");
}

/// **The false positive that matters most.** An untyped function reads a local
/// of the function that called it, which is legal, driven, and the reason the
/// fork exists. Reporting it would make the check worse than the silence.
#[test]
fn a_caller_s_local_is_in_scope_and_stays_silent() {
    let found = problems(
        "<template><screen><text>hi</text></screen></template>\
         <script>let n = signal(0);\n\
         fn outer() {\nlet helper_local = 42;\ninner(1);\n}\n\
         fn inner(by) {\nn = helper_local + by;\n}</script>",
    );
    assert!(
        !found.contains("helper_local"),
        "a caller's local is in scope, and saying otherwise flags working code: {found}"
    );
}

/// A typed function does not get that scope, and one with no parameters is
/// typed (`docs/10-types.md`). That is the type checker's to say, in its own
/// words; the undefined-name check still says nothing, since the name exists.
#[test]
fn a_typed_function_reading_a_caller_s_local_is_the_type_checker_s_error() {
    let found = problems(
        "<template><screen><text>hi</text></screen></template>\
         <script>let n = signal(0);\n\
         fn outer() {\nlet helper_local = 42;\ninner();\n}\n\
         fn inner() {\nn = helper_local;\n}</script>",
    );
    assert!(!found.contains("declared nowhere"), "the name exists: {found}");
    assert!(found.contains("`inner` reads `helper_local`"), "a typed fn cannot read it: {found}");
}

/// A handler is a caller too, so its `let`s reach whatever it calls.
#[test]
fn a_handler_s_local_is_in_scope() {
    let found = problems(
        "<template><screen><view @tap=\"let picked = 3; use_it();\"><text>go</text></view>\
         </screen></template>\
         <script>let n = signal(0);\nfn use_it() {\nn = picked;\n}</script>",
    );
    assert!(!found.contains("declared nowhere"), "a handler's local reaches what it calls: {found}");
    // `use_it` has no parameters, so it is typed and may not rely on that.
    assert!(found.contains("`use_it` reads `picked`"), "{found}");
}

/// An `r-for` binds a name for the whole row, including inside anything the
/// row's handler calls.
#[test]
fn a_loop_variable_is_in_scope() {
    let found = problems(
        "<template><screen>\
         <view r-for=\"item in items\" r-key=\"item.id\" @tap=\"pick()\"><text>x</text></view>\
         </screen></template>\
         <script>let items = signal([]);\nlet chosen = signal(0);\n\
         fn pick() {\nchosen = item.id;\n}</script>",
    );
    assert!(!found.contains("declared nowhere"), "a loop variable is in scope in the row: {found}");
    // `pick` has no parameters, so it is typed and may not rely on that.
    assert!(found.contains("`pick` reads `item`"), "{found}");
}

/// The document's own signals are the ordinary case, and a parameter is the
/// other one.
#[test]
fn signals_and_parameters_are_in_scope() {
    let found = problems(
        "<template><screen><text>hi</text></screen></template>\
         <script>let total = signal(0);\nfn add(amount) {\ntotal = total + amount;\n}</script>",
    );
    assert!(!found.contains("declared nowhere"), "nothing to report here: {found}");
}

/// A name that is a function is the other check's to report, and saying it
/// twice in different words helps nobody.
#[test]
fn a_function_name_is_not_reported_as_an_undeclared_variable() {
    let found = problems(
        "<template><screen><text>hi</text></screen></template>\
         <script>let n = signal(0);\nfn one() {\ntwo();\n}\nfn two() {\nn = 1;\n}</script>",
    );
    assert!(!found.contains("declared nowhere"), "both functions exist: {found}");
}

/// It is an **error**, and it carries the line the name is read on.
///
/// The severity is the user's call, 2026-09-17, made while looking at the
/// squiggle: a name declared nowhere is a mistake, and calling it a warning let
/// `rux check` exit 0 on a document that cannot work. The line matters as much:
/// reported without one it was drawn at the top of the file, pointing at
/// `<template>` for a mistake in `<script>`.
#[test]
fn it_is_an_error_and_it_says_where() {
    let doc = Document::from_source(
        "<template><screen><text>hi</text></screen></template>
         <script>let n = signal(0);
fn add() {
Have
}</script>",
    )
    .expect("loads");
    let mine: Vec<_> = doc
        .diagnostics()
        .warnings
        .iter()
        .filter(|w| w.message.contains("declared nowhere"))
        .collect();
    assert_eq!(mine.len(), 1, "exactly one report");
    assert!(mine[0].is_error(), "a name nothing declares is a mistake, not a caution");
    assert!(mine[0].line.is_some(), "and it has to say which line reads it");
}
