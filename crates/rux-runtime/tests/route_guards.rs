//! Route guards: what stands between a navigation and the page it wants.
//!
//! Guards run **before the history moves**, which is the whole point of them
//! being here rather than in a page: a cancelled navigation must leave no entry
//! behind and must not open a route transition, and by the time a page could
//! refuse to render itself both have already happened.
//!
//! The answers are vue-router's, because that is where the mental model comes
//! from: `false` cancels, a string redirects, anything else allows.

use std::fs;
use std::path::PathBuf;

use rux_runtime::Document;

/// Write a little app into a scratch directory and load it.
///
/// `components` is `(name, body)`, each becoming `components/<name>.rux`.
fn app(components: &[(&str, &str)], template: &str, script: &str) -> Document {
    let dir: PathBuf = std::env::temp_dir().join(format!(
        "rux_guards_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(dir.join("components")).unwrap();
    for (name, body) in components {
        fs::write(dir.join("components").join(format!("{name}.rux")), body).unwrap();
    }
    let uses: String =
        components.iter().map(|(n, _)| format!("use components::{n};\n")).collect();
    fs::write(
        dir.join("app.rux"),
        format!("<template><screen>{template}</screen></template>\n<script>\n{uses}{script}</script>"),
    )
    .unwrap();
    let doc = Document::load(dir.join("app.rux")).expect("loads");
    let _ = fs::remove_dir_all(&dir);
    doc
}

const HOME: &str = r#"<template><view class="page"><text>home</text></view></template>"#;
const ABOUT: &str = r#"<template><view class="page"><text>about</text></view></template>"#;

/// A router with `/` open and `/about` behind a guard called `gate`.
fn gated() -> Document {
    app(
        &[("home", HOME), ("about", ABOUT)],
        r#"<router>
             <route path="/" view="home" />
             <route path="/about" view="about" guard="gate" />
           </router>"#,
        "let gate = signal(true);\n",
    )
}

/// The three answers, in one test.
#[test]
fn a_guard_can_refuse_redirect_or_allow() {
    let mut doc = gated();
    assert!(doc.navigate("/about"), "allowed while the guard says true");
    assert_eq!(doc.route(), "/about");

    assert!(doc.navigate("/"), "back to the start");
    assert!(doc.apply_handler("gate = false"), "shut the gate");
    assert!(!doc.navigate("/about"), "refused");
    assert_eq!(doc.route(), "/", "and nothing moved");
}

/// A guard that names a path sends the navigation there instead, and the page
/// it refused never enters the history, so Back does not walk into it.
#[test]
fn a_guard_can_redirect_and_leaves_no_entry_behind() {
    let mut doc = app(
        &[("home", HOME), ("about", ABOUT), ("login", ABOUT)],
        r#"<router>
             <route path="/" view="home" />
             <route path="/login" view="login" />
             <route path="/about" view="about" guard="gate" />
           </router>"#,
        "let gate = signal(true);\n",
    );
    assert!(doc.apply_handler(r#"gate = "/login""#), "the guard now redirects");
    assert!(doc.navigate("/about"), "the navigation happened");
    assert_eq!(doc.route(), "/login", "just not to where it was aimed");

    assert!(doc.back(), "and Back goes to where we started");
    assert_eq!(doc.route(), "/", "not to the page that was refused");
}

/// An answer that is neither `false` nor a path allows, and that deliberately
/// includes `()`.
///
/// `()` is what a guard body with no explicit answer evaluates to, and what an
/// `if` with no `else` returns. Treating an absent answer as a refusal would
/// make the natural shape, `if !user { return "/login"; }`, block every
/// navigation it did not object to.
#[test]
fn a_guard_with_nothing_to_say_allows() {
    let mut doc = app(
        &[("home", HOME), ("about", ABOUT)],
        r#"<router>
             <route path="/" view="home" />
             <route path="/about" view="about" guard="check()" />
           </router>"#,
        "let seen = signal(0);\nfn check() { seen++; }\n",
    );
    assert!(doc.navigate("/about"), "an empty answer is not a refusal");
    assert_eq!(doc.route(), "/about");
}

/// **A guard that only runs on `navigate` protects nothing**, because Back
/// reaches the same page without going through it, and Back is how anyone
/// leaves a login screen.
///
/// This is the half of route guards that is easy to leave out and hard to
/// notice, because every test written by hand navigates forwards.
#[test]
fn back_and_forward_go_through_the_guard_too() {
    let mut doc = gated();
    assert!(doc.navigate("/about"), "in while the gate is open");
    // `back`, not `navigate("/")`: navigating anywhere pushes, which drops what
    // was ahead, and then there is no Forward left to refuse.
    assert!(doc.back(), "and out again, so there is something to go forward to");

    assert!(doc.apply_handler("gate = false"), "shut it");
    assert!(!doc.forward(), "Forward is refused as well");
    assert_eq!(doc.route(), "/", "and did not move");

    assert!(doc.apply_handler("gate = true"), "open it");
    assert!(doc.forward(), "Forward works again");
    assert_eq!(doc.route(), "/about");

    assert!(doc.apply_handler("gate = false"), "shut it behind us");
    assert!(doc.back(), "leaving a guarded page is not the guard's business");
    assert_eq!(doc.route(), "/");
}

/// A deep link is the case an auth guard most needs to catch: arriving straight
/// at a path from a URL bar or `--route` goes through no link in the app.
#[test]
fn a_deep_link_is_guarded_too() {
    let mut doc = gated();
    assert!(doc.apply_handler("gate = false"), "shut the gate");
    doc.start_at("/about");
    assert_eq!(doc.route(), "/", "a refused arrival lands at the root");
}

/// A guard on a parent route covers every page inside it, without being written
/// on each one.
#[test]
fn a_section_guard_covers_what_is_under_it() {
    let mut doc = app(
        &[
            ("home", HOME),
            ("shell", r#"<template><view class="page"><router-view /></view></template>"#),
            ("one", r#"<template><view class="page"><text>{{ id }}</text></view></template>"#),
        ],
        r#"<router>
             <route path="/" view="home" />
             <route path="/crew" view="shell" guard="gate">
               <route path=":id" view="one" />
             </route>
           </router>"#,
        "let gate = signal(true);\n",
    );
    assert!(doc.navigate("/crew/ada"), "the child is reachable");
    assert!(doc.navigate("/"), "back out");

    assert!(doc.apply_handler("gate = false"), "shut the section");
    assert!(!doc.navigate("/crew/ada"), "the child is covered by its parent's guard");
    assert_eq!(doc.route(), "/");
}

/// A guard reads the parameters its own level captured, so it can decide about
/// the row rather than only about the section.
#[test]
fn a_guard_sees_what_its_level_captured() {
    let mut doc = app(
        &[("home", HOME), ("one", ABOUT)],
        r#"<router>
             <route path="/" view="home" />
             <route path="/crew/:id" view="one" guard="id != &quot;secret&quot;" />
           </router>"#,
        "",
    );
    assert!(doc.navigate("/crew/ada"), "an ordinary member is fine");
    assert!(!doc.navigate("/crew/secret"), "that one is not");
    assert_eq!(doc.route(), "/crew/ada", "and the refusal left us where we were");
}

/// A guard on the `<router>` itself runs on every navigation, and runs before
/// any route's own, so the coarse question is answered first.
#[test]
fn a_router_guard_runs_on_every_navigation() {
    let mut doc = app(
        &[("home", HOME), ("about", ABOUT)],
        r#"<router guard="open">
             <route path="/" view="home" />
             <route path="/about" view="about" />
           </router>"#,
        "let open = signal(true);\n",
    );
    assert!(doc.navigate("/about"), "through while it is open");
    assert!(doc.apply_handler("open = false"), "close everything");
    assert!(!doc.navigate("/"), "even the root is refused");
    assert_eq!(doc.route(), "/about");
}

/// Guards that send each other in a circle are cut off and reported rather than
/// hanging the window. Same bound, and the same reason, as an `emit` chain.
#[test]
fn a_circle_of_redirects_is_cut_off() {
    let mut doc = app(
        &[("home", HOME), ("about", ABOUT)],
        r#"<router>
             <route path="/" view="home" guard="&quot;/about&quot;" />
             <route path="/about" view="about" guard="&quot;/&quot;" />
           </router>"#,
        "",
    );
    assert!(!doc.navigate("/about"), "refused rather than looping");
    assert!(
        doc.diagnostics().warnings.iter().any(|w| w.message.contains("circle")),
        "and said why: {:?}",
        doc.diagnostics().warnings
    );
}

/// A guard naming the path it was asked about has allowed it, whatever it meant
/// to say. Following it would be the shortest possible loop.
#[test]
fn a_guard_that_redirects_to_itself_is_an_allow() {
    let mut doc = app(
        &[("home", HOME), ("about", ABOUT)],
        r#"<router>
             <route path="/" view="home" />
             <route path="/about" view="about" guard="&quot;/about&quot;" />
           </router>"#,
        "",
    );
    assert!(doc.navigate("/about"), "allowed");
    assert_eq!(doc.route(), "/about");
}

/// A broken guard is reported **at load**, on the same terms as a `@tap`.
///
/// It hides longer than a handler does: nobody taps a guard, so a broken one is
/// found by whoever navigates, and what they see is a link that does nothing.
#[test]
fn a_guard_that_cannot_compile_is_reported_at_load() {
    let doc = app(
        &[("home", HOME), ("about", ABOUT)],
        r#"<router>
             <route path="/" view="home" />
             <route path="/about" view="about" guard="unlocked ? 1 : 2" />
           </router>"#,
        "let unlocked = signal(false);\n",
    );
    assert!(
        doc.diagnostics().warnings.iter().any(|w| w.message.contains("guard")),
        "reported without anyone navigating: {:?}",
        doc.diagnostics().warnings
    );
}
