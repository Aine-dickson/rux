//! Every shipped example must load, and load *clean*.
//!
//! Now that the dev overlay shows warnings in the window, a noisy example is a
//! visible defect: open it and a panel covers the demo. This walks `examples/`
//! and fails with the offending file and message, so the examples stay the
//! reference for what good `.rux` looks like.

use std::path::{Path, PathBuf};

use rux_runtime::Document;

fn examples_dir() -> PathBuf {
    // Tests run with the crate root as the working directory.
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn example_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(examples_dir())
        .expect("examples/ is readable")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension()? == "rux").then_some(path)
        })
        .collect();
    files.sort();
    assert!(!files.is_empty(), "found no examples to check");
    files
}

/// Drive the computed/effect example the way a person would, and check the
/// numbers it puts on screen.
///
/// The suite already proves every example *loads*; this proves one of them
/// *works*, which for a reactivity feature is the part that can quietly rot.
#[test]
fn the_computed_example_recomputes_when_tapped() {
    fn texts(node: &rux_layout::Node) -> Vec<String> {
        let mut out: Vec<String> = node.text.iter().map(|t| t.text.clone()).collect();
        for child in &node.children {
            out.extend(texts(child));
        }
        out
    }
    let has = |doc: &Document, needle: &str| texts(&doc.root).iter().any(|t| t == needle);

    let mut doc = Document::load(examples_dir().join("computed.rux")).expect("loads");
    // qty 2 x price 12 → 24, tax 2.4, total 26.4, and the effect has run once.
    assert!(has(&doc, "24"), "subtotal on load: {:?}", texts(&doc.root));
    assert!(has(&doc, "26.4"), "total on load: {:?}", texts(&doc.root));
    assert!(
        texts(&doc.root).iter().any(|t| t.contains("within budget")),
        "the effect ran on load, so the status is not blank: {:?}",
        texts(&doc.root)
    );

    // Tap `+` eight times: 10 x 12 = 120, over the 100 budget.
    for _ in 0..8 {
        assert!(doc.apply_handler("qty = qty + 1"), "the tap changed state");
    }
    assert!(has(&doc, "120"), "subtotal followed: {:?}", texts(&doc.root));
    assert!(has(&doc, "132"), "and so did the computed that reads it");
    assert!(
        texts(&doc.root).iter().any(|t| t.contains("over budget")),
        "the effect re-ran and flipped the status: {:?}",
        texts(&doc.root)
    );
}

/// Drive the router example the way a person would: tap a link, follow a row
/// into a detail page, and come back.
///
/// The links matter as much as the routes. A `to=` that produced no tappable
/// region would leave a router that only the API can drive, which is a router
/// nobody can use.
#[test]
fn the_router_example_navigates() {
    fn texts(node: &rux_layout::Node) -> Vec<String> {
        let mut out: Vec<String> = node.text.iter().map(|t| t.text.clone()).collect();
        for child in &node.children {
            out.extend(texts(child));
        }
        out
    }
    fn find_link<'a>(node: &'a rux_layout::Node, label: &str) -> Option<&'a rux_layout::Node> {
        let names_it = node.text.as_ref().is_some_and(|t| t.text.trim() == label);
        if names_it && node.on_tap.is_some() {
            return Some(node);
        }
        // A link may be a box around the text, so the handler is on the parent.
        if node.on_tap.is_some() && texts(node).iter().any(|t| t.trim() == label) {
            return Some(node);
        }
        node.children.iter().find_map(|c| find_link(c, label))
    }
    let has = |doc: &Document, needle: &str| {
        texts(&doc.root).iter().any(|t| t.contains(needle))
    };

    let mut doc = Document::load(examples_dir().join("router.rux")).expect("loads");
    assert!(has(&doc, "a router, at last"), "the home page: {:?}", texts(&doc.root));

    // The nav bar is a component, and its links are ordinary tappable nodes.
    let crew = find_link(&doc.root, "crew").expect("a crew link").clone();
    assert_eq!(crew.access.role, rux_layout::AccessRole::Link, "announced as a link");
    assert!(doc.apply_handler_in(&crew.on_tap.clone().unwrap(), crew.instance.as_deref()));
    assert_eq!(doc.route(), "/crew");
    assert!(has(&doc, "Grace"), "the list rendered: {:?}", texts(&doc.root));

    // A row's `:to` is computed from the row itself, so each goes somewhere else.
    let row = find_link(&doc.root, "Grace").expect("a crew row").clone();
    assert!(doc.apply_handler_in(&row.on_tap.clone().unwrap(), row.instance.as_deref()));
    assert_eq!(doc.route(), "/crew/grace");
    assert!(has(&doc, "engineer"), "the detail page looked her up: {:?}", texts(&doc.root));

    // `back()` from inside the page walks the history rather than linking.
    let back = find_link(&doc.root, "back").expect("a back button").clone();
    assert!(doc.apply_handler_in(&back.on_tap.clone().unwrap(), back.instance.as_deref()));
    assert_eq!(doc.route(), "/crew");
    assert!(has(&doc, "Grace"), "and the list is back: {:?}", texts(&doc.root));

    // Nothing matches, so the fallback renders, and the view reads `route` for
    // itself: the path is a signal, and a route's view is not cut off from it.
    doc.navigate("/nowhere");
    assert!(has(&doc, "nothing here"), "the fallback: {:?}", texts(&doc.root));
    assert!(has(&doc, "/nowhere"), "which read the path: {:?}", texts(&doc.root));

    // `params` is read in the document's own footer, outside the matched view,
    // which is the whole reason it exists. It empties when a route captures
    // nothing rather than keeping the last page's answer.
    assert!(!has(&doc, "viewing:"), "no parameters here: {:?}", texts(&doc.root));
    doc.navigate("/crew/hedy");
    assert!(has(&doc, "viewing: hedy"), "read outside the view: {:?}", texts(&doc.root));

    // And the history buttons say whether they lead anywhere. Compared against
    // each other rather than against a colour written here twice: what matters
    // is that a dead button does not look like a live one.
    let paint = |doc: &Document, label: &str| {
        let button = find_link(&doc.root, label).expect("a history button");
        format!("{:?}", button.style.background)
    };
    assert_ne!(
        paint(&doc, "go back"),
        paint(&doc, "go forward"),
        "plenty behind us and nothing ahead, so the two should not look alike"
    );
    doc.back();
    assert_eq!(paint(&doc, "go back"), paint(&doc, "go forward"), "both lead somewhere now");
}

#[test]
fn every_example_loads() {
    let mut failures = Vec::new();
    for path in example_files() {
        if let Err(err) = Document::load(&path) {
            failures.push(format!("{}: {err}", path.display()));
        }
    }
    assert!(failures.is_empty(), "examples failed to load:\n{}", failures.join("\n"));
}

#[test]
fn every_example_is_warning_free() {
    let mut noisy = Vec::new();
    for path in example_files() {
        let Ok(doc) = Document::load(&path) else { continue }; // reported by the test above
        let warnings = &doc.diagnostics().warnings;
        if !warnings.is_empty() {
            let listed: Vec<String> = warnings.iter().map(|w| w.to_string()).collect();
            noisy.push(format!("{}:\n  - {}", path.display(), listed.join("\n  - ")));
        }
    }
    assert!(
        noisy.is_empty(),
        "examples raise warnings the dev overlay will show:\n{}",
        noisy.join("\n")
    );
}

/// The keyed-list example's `order()` and `rotate()` still say what they said.
///
/// Both were rewritten in v0.7 to use the JS-named collection methods and an
/// arrow function, which turned a six-line loop into one line. `rux check` only
/// reports warnings, so nothing else in the suite would notice if the shorter
/// version quietly produced a different string, and the whole point of the
/// example is that rows keep their identity across a reorder.
#[test]
fn the_keyed_list_example_still_orders_correctly() {
    fn texts(node: &rux_layout::Node) -> Vec<String> {
        let mut out: Vec<String> = node.text.iter().map(|t| t.text.clone()).collect();
        for child in &node.children {
            out.extend(texts(child));
        }
        out
    }
    let order = |doc: &Document| {
        texts(&doc.root)
            .into_iter()
            .find(|t| t.starts_with("Order: "))
            .expect("the footer line")
    };

    let mut doc = Document::load(examples_dir().join("keyed-list.rux")).expect("loads");
    assert_eq!(order(&doc), "Order: one, two, three");

    // Rotating moves the last row to the front, and `order()` reports it.
    let tap = {
        fn find(node: &rux_layout::Node) -> Option<&rux_layout::Node> {
            if node.on_tap.is_some() {
                return Some(node);
            }
            node.children.iter().find_map(find)
        }
        find(&doc.root).expect("a rotate button").on_tap.clone().unwrap()
    };
    assert!(doc.apply_handler(&tap));
    assert_eq!(order(&doc), "Order: three, one, two");
}
