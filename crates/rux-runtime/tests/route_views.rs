//! How a `<route>` names its view, and what it says when the name is wrong.
//!
//! A view is named the way its tag is, in kebab, while the `use` that brings it
//! in names the file, in snake. Two spellings for one component is a place an
//! author will land, so the two ways of landing there both have to say
//! something true.

use std::fs;
use std::path::PathBuf;

use rux_runtime::Document;

/// Distinguishes two scratch directories created inside one clock tick. See the
/// comment where it is used.
static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Write a little app into a scratch directory and load it.
///
/// The `use` lines are given explicitly, because whether a component is
/// imported is the thing under test.
fn app(components: &[(&str, &str)], template: &str, script: &str) -> Document {
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "rux_views_{}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        // A counter as well as the clock, because the clock is not enough.
        // Windows ticks its system time about every 15ms, so `as_nanos()` gives
        // two tests that start in the same tick the *same* number; with the same
        // pid that is the same directory, and the first to finish deletes the
        // other's files out from under a load that is still running. It shows up
        // as one unrelated test failing about one run in six, with a different
        // name each time, which is the least debuggable shape a flake has.
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    ));
    fs::create_dir_all(dir.join("components")).unwrap();
    for (name, body) in components {
        fs::write(dir.join("components").join(format!("{name}.rux")), body).unwrap();
    }
    fs::write(
        dir.join("app.rux"),
        format!(
            "<template><screen>{template}</screen></template>\n<script>\n{script}</script>"
        ),
    )
    .unwrap();
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let _ = fs::remove_dir_all(&dir);
    doc
}

const PAGE: &str = r#"<template><view class="page"><text>page</text></view></template>"#;

fn problems(doc: &Document) -> String {
    // Unescaped: the Debug form doubles every quote in the message, and these
    // messages are about the quotes.
    format!("{:?}", doc.diagnostics()).replace('\\', "")
}

/// The spelling that works, which is the control the other two are worth
/// nothing without.
#[test]
fn a_view_named_the_way_its_tag_is_renders() {
    let doc = app(
        &[("page_a", PAGE)],
        r#"<router><route path="/" view="page-a" /></router>"#,
        "use components::page_a;\n",
    );
    let said = problems(&doc);
    assert!(!said.contains("<route> names the view"), "nothing to say: {said}");
}

/// The import's own spelling works, and is not a mistake to be corrected.
///
/// One component has two names and the author chose neither. `use pages::new_task;`
/// has to be snake, because it names a file; the tag it contributes has to be
/// kebab, because that is what a custom element looks like. The author was left
/// holding the difference, and `view="new_task"` — a name they had already given
/// correctly two lines below — was told to go and write the other spelling of it.
///
/// Reported by the user 2026-09-15: "Why am I forced to write new_task as
/// new-task in view=?". The answer was that they should not be.
#[test]
fn a_view_written_in_the_import_spelling_renders() {
    let doc = app(
        &[("page_a", PAGE)],
        r#"<router><route path="/" view="page_a" /></router>"#,
        "use components::page_a;
",
    );
    let said = problems(&doc);
    assert!(!said.contains("<route> names the view"), "nothing to say: {said}");
    assert!(!said.contains("is not imported"), "and certainly not that: {said}");
}

/// A component tag takes either spelling too, for the same reason.
#[test]
fn a_component_tag_may_be_written_either_way() {
    let doc = app(
        &[("page_a", PAGE)],
        "<page_a /><page-a />",
        "use components::page_a;
",
    );
    let said = problems(&doc);
    assert!(!said.contains("page_a"), "the underscored tag resolves: {said}");
}

/// A view nobody imported is still told to import it, and told a name that is
/// a name.
///
/// The suggestion used to echo the view's own hyphens, so pasting
/// `use components::page-a;` turned a warning into a hard error looking for a
/// `page-a.rux` that no convention here would ever produce.
#[test]
fn an_unimported_view_is_suggested_an_import_that_parses() {
    let doc = app(
        &[],
        r#"<router><route path="/" view="page-a" /></router>"#,
        "let x = signal(0);\n",
    );
    let said = problems(&doc);
    assert!(said.contains("which is not imported"), "still the missing-import case: {said}");
    assert!(said.contains("use components::page_a;"), "in the file's spelling: {said}");
    assert!(!said.contains("use components::page-a;"), "never the tag's: {said}");
}

/// A route you are not standing on is checked too.
///
/// This is the case that sent someone here: `<route path="/new" view="new_task" />`
/// with no `use` for it loaded clean, `rux check` exited 0, and the mistake
/// waited until the first navigation to `/new`. Whether a name is imported is a
/// fact about the file, not about which route matched.
#[test]
fn a_route_that_did_not_match_is_still_checked() {
    let doc = app(
        &[("home", PAGE)],
        r#"<router><route path="/" view="home" /><route path="/new" view="new_task" /></router>"#,
        "use components::home;\n",
    );
    let said = problems(&doc);
    assert!(
        said.contains("names the view `new_task`"),
        "the unvisited route is reported: {said}"
    );
}

/// And it is an error, not a warning: the page can never render.
#[test]
fn an_unimported_view_is_an_error() {
    let doc = app(
        &[],
        r#"<router><route path="/" view="page-a" /></router>"#,
        "let x = signal(0);\n",
    );
    assert!(
        doc.diagnostics().warnings.iter().any(|w| w.is_error() && w.message.contains("page-a")),
        "raised as an error: {:?}",
        doc.diagnostics().warnings
    );
}

/// Reported once, against the line the `view=` was written on.
///
/// The matched route is checked twice now, once by the load-time pass and once
/// where it fails to expand. Both say the same thing from the same line, which
/// is what the sink's dedupe is for; if they ever drift apart, an author gets
/// the same mistake twice.
#[test]
fn the_matched_route_is_not_reported_twice() {
    let doc = app(
        &[],
        r#"<router><route path="/" view="page-a" /></router>"#,
        "let x = signal(0);\n",
    );
    let hits: Vec<_> = doc
        .diagnostics()
        .warnings
        .iter()
        .filter(|w| w.message.contains("names the view `page-a`"))
        .collect();
    assert_eq!(hits.len(), 1, "said once: {hits:?}");
    assert_eq!(hits[0].line, Some(1), "on the line the `view=` is on: {hits:?}");
}

/// A `to=` naming a path no route answers to.
///
/// Dead by construction and silent by construction: tapping navigates, the
/// router matches nothing, and the screen goes blank with no explanation. Both
/// halves are written in the markup, so they can be compared before anyone taps.
#[test]
fn a_link_to_nowhere_is_reported() {
    let doc = app(
        &[("home", PAGE)],
        r#"<text to="/typo">x</text><router><route path="/" view="home" /></router>"#,
        "use components::home;\n",
    );
    let said = problems(&doc);
    assert!(said.contains(r#"`to="/typo"` matches no <route>"#), "reported: {said}");
}

/// A link that does reach a route, including through a `:param`, says nothing.
#[test]
fn a_link_that_reaches_a_route_is_left_alone() {
    let doc = app(
        &[("home", PAGE), ("detail", PAGE)],
        r#"<text to="/">a</text><text to="/task/7">b</text><text to="/task/7?tab=notes">c</text>
           <router><route path="/" view="home" /><route path="/task/:id" view="detail" /></router>"#,
        "use components::home;\nuse components::detail;\n",
    );
    let said = problems(&doc);
    assert!(!said.contains("matches no <route>"), "nothing to say: {said}");
}

/// A `<route fallback>` catches everything, so nothing is a dead link.
///
/// The first draft of this check compared against a flattened list of patterns
/// and called `examples/router.rux` broken: that file links to `/nowhere` on
/// purpose, to show the fallback working. Running through the real matcher is
/// what makes the check agree with the router.
#[test]
fn a_fallback_route_means_no_link_is_dead() {
    let doc = app(
        &[("home", PAGE), ("lost", PAGE)],
        r#"<text to="/nowhere">x</text>
           <router><route path="/" view="home" /><route fallback view="lost" /></router>"#,
        "use components::home;\nuse components::lost;\n",
    );
    let said = problems(&doc);
    assert!(!said.contains("matches no <route>"), "the fallback answers it: {said}");
}

/// A document with no router at all says nothing about its links.
///
/// A component holds links and no router of its own, and `rux check` is run on
/// components. Reporting every one of them would be a check nobody could leave
/// switched on.
#[test]
fn a_document_without_a_router_reports_no_dead_links() {
    let doc = app(&[], r#"<text to="/anywhere">x</text>"#, "let x = signal(0);\n");
    let said = problems(&doc);
    assert!(!said.contains("matches no <route>"), "nothing to check against: {said}");
}

/// An `<input>` with no `r-model` is inert, and now says so.
///
/// Reported 2026-09-15 as "my input elements are uninteractive". The layout
/// only makes a focus region for an input that carries a model
/// (`if let Some(model) = &node.model` in `rux-layout`), so without one the box
/// paints, the placeholder renders, and a tap reaches nothing at all.
#[test]
fn an_input_with_no_model_is_reported() {
    let doc = app(&[], r#"<input placeholder="Task title" />"#, "let x = signal(0);\n");
    let said = problems(&doc);
    assert!(said.contains("has no `r-model`"), "reported: {said}");
    assert!(said.contains("level: Error"), "as an error, since it can never work: {said}");
}

/// **A `type=` Rux does not know is refused, and `password` is why.**
///
/// Four values were acted on; anything else fell through to the plain text
/// path with nothing said. Asked by the user 2026-09-21, and the honest answer
/// was that `<input type="password">` rendered a working field that showed
/// every character typed into it. The author had written the one thing that
/// says "hide this" and the engine had ignored it.
///
/// `password` is real now, so this uses `email`, which is not: it is a
/// keyboard hint rather than a control, and is deliberately still refused.
#[test]
fn an_input_type_rux_does_not_know_is_refused() {
    let doc = app(
        &[],
        r#"<input type="email" r-model="pw" />"#,
        "let pw = signal(\"\");
",
    );
    let said = problems(&doc);
    assert!(said.contains("is not a kind of input"), "reported: {said}");
    assert!(said.contains("level: Error"), "as an error, not a warning: {said}");
    // The message names the way out, not just the problem.
    assert!(said.contains("textarea"), "and lists what is allowed: {said}");
}

/// Every kind Rux does have is left alone, including an absent one.
#[test]
fn the_input_types_rux_has_are_accepted() {
    for kind in ["text", "textarea", "password", "search", "checkbox", "radio"] {
        let doc = app(
            &[],
            &format!(r#"<input type="{kind}" r-model="v" />"#),
            "let v = signal(\"\");
",
        );
        let said = problems(&doc);
        assert!(!said.contains("is not a kind of input"), "{kind} is real: {said}");
    }
    // `select` wants its options, so it is written the way an author would.
    let doc = app(
        &[],
        r#"<input type="select" r-model="v" :options="opts" />"#,
        "let v = signal(\"a\");
let opts = signal([\"a\", \"b\"]);
",
    );
    let said = problems(&doc);
    assert!(!said.contains("is not a kind of input"), "select is real: {said}");
    // And no `type` at all is a text field, which is the common case.
    let doc = app(&[], r#"<input r-model="v" />"#, "let v = signal(\"\");
");
    let said = problems(&doc);
    assert!(!said.contains("is not a kind of input"), "absent is text: {said}");
}

/// And a bound one says nothing.
#[test]
fn a_bound_input_is_left_alone() {
    let doc = app(
        &[],
        r#"<input r-model="title" placeholder="Task title" />"#,
        "let title = signal(\"\");\n",
    );
    let said = problems(&doc);
    assert!(!said.contains("has no `r-model`"), "nothing to say: {said}");
}
