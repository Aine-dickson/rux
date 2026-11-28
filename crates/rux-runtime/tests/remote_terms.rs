//! The terms a remote document is opened on: what an opener may hand out.

use rux_runtime::window_open::remote_terms;
use rux_runtime::Sandbox;

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_trusted_opener_may_grant_any_known_name_but_not_its_own_stores() {
    let terms = remote_terms(None, &names(&["clipboard", "links.open@example.com"])).expect("granted");
    assert!(terms.grants("clipboard"));
    assert!(terms.names("links"));
    let why = remote_terms(None, &names(&["settings"])).unwrap_err();
    assert!(why.contains("the app's own"), "{why}");
    let why = remote_terms(None, &names(&["secrets.get"])).unwrap_err();
    assert!(why.contains("the app's own"), "{why}");
    let why = remote_terms(None, &names(&["nonsense"])).unwrap_err();
    assert!(why.contains("`nonsense`"), "{why}");
}

#[test]
fn no_grants_is_no_grants_and_a_trusted_opener_gives_default_limits() {
    let terms = remote_terms(None, &[]).expect("none");
    assert!(terms.grants.is_empty());
    assert_eq!(terms.limits.max_source_bytes, Sandbox::new().limits.max_source_bytes);
}

#[test]
fn a_sandboxed_opener_cannot_hand_out_more_than_it_holds() {
    let opener = Sandbox::new().grant("clipboard").grant("links");
    assert!(remote_terms(Some(&opener), &names(&["clipboard"])).is_ok());
    // A module granted whole covers a function or a host of it.
    assert!(remote_terms(Some(&opener), &names(&["links.open@example.com"])).is_ok());
    let why = remote_terms(Some(&opener), &names(&["files"])).unwrap_err();
    assert!(why.contains("`files`") && why.contains("opener"), "{why}");
    // A function does not cover its module.
    let narrow = Sandbox::new().grant("links.open");
    assert!(remote_terms(Some(&narrow), &names(&["links"])).is_err());
}

#[test]
fn a_sandboxed_opener_passes_its_limits_on() {
    let mut opener = Sandbox::new();
    opener.limits.max_source_bytes = 1234;
    let terms = remote_terms(Some(&opener), &[]).expect("none");
    assert_eq!(terms.limits.max_source_bytes, 1234);
}
