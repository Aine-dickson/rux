//! A route is input anyone can send. With a `scheme` or `link-hosts`, any app
//! on the phone and any web page can open the app on any route, and it used to
//! arrive exactly as an in-app tap would: a guard had `to` and `from` and
//! nothing that told the two apart. `linked` does.

use rux_runtime::Document;

const APP: &str = r#"<template><screen><router>
  <route path="/" view="home" />
  <route path="/delete" view="home" guard='if linked { "/confirm" }' />
  <route path="/confirm" view="home" />
</router></screen></template>
<script>
use components::home;
</script>"#;

fn load() -> Document {
    let dir = std::env::temp_dir().join(format!("rux_links_{}", std::process::id()));
    std::fs::create_dir_all(dir.join("components")).unwrap();
    std::fs::write(dir.join("app.rux"), APP).unwrap();
    std::fs::write(
        dir.join("components/home.rux"),
        "<template><view><text>home</text></view></template>",
    )
    .unwrap();
    Document::load(dir.join("app.rux")).expect("loads")
}

#[test]
fn a_guard_can_tell_a_link_from_a_tap() {
    let mut doc = load();
    assert!(doc.navigate("/delete"), "a tap inside the app goes where it says");
    assert_eq!(doc.route(), "/delete");

    let mut doc = load();
    doc.open_link("/delete", true);
    assert_eq!(doc.route(), "/confirm", "a link to the same place is sent to ask first");

    let mut doc = load();
    doc.open_link("/delete", false);
    assert_eq!(doc.route(), "/confirm", "and a cold start by link too");
}

#[test]
fn linked_is_only_true_for_the_link() {
    let mut doc = load();
    doc.open_link("/", false);
    assert!(doc.navigate("/delete"), "the next tap is a tap again");
    assert_eq!(doc.route(), "/delete");
}
