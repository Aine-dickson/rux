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

fn rux_files_in(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{} is readable: {e}", dir.display()))
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension()? == "rux").then_some(path)
        })
        .collect();
    files.sort();
    files
}

/// Every app in `examples/`, plus the recipes, which live in a directory of
/// their own and were not covered until they were added here.
///
/// Only apps: `components/` is skipped in both places, since a component file
/// is not a document and does not load as one.
fn example_files() -> Vec<PathBuf> {
    let mut files = rux_files_in(&examples_dir());
    files.extend(rux_files_in(&examples_dir().join("recipes")));
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

/// Drive the element-query example the way a person would, so the feature it
/// demonstrates is proven rather than merely parsed.
///
/// `rux check` loads every example but runs no handler, and every interesting
/// thing here happens in one. Without this, the example could go on checking
/// clean long after `query()` stopped answering.
#[test]
fn the_element_query_example_measures_and_focuses() {
    fn texts(node: &rux_layout::Node) -> Vec<String> {
        let mut out: Vec<String> = node.text.iter().map(|t| t.text.clone()).collect();
        for child in &node.children {
            out.extend(texts(child));
        }
        out
    }
    let said = |doc: &Document, needle: &str| {
        texts(&doc.root).iter().any(|t| t.contains(needle))
    };

    let mut doc = Document::load(examples_dir().join("element-query.rux")).expect("loads");

    // Counting needs no layout: it is a fact about the tree, not the frame.
    assert!(doc.apply_handler("count()"), "the tap changed state");
    assert!(said(&doc, "2 cards"), "counted them: {:?}", texts(&doc.root));
    assert!(said(&doc, ".card.wide"), "and read the classes back");
    assert!(said(&doc, "id two"), "and the id");

    // Measuring does need one. Stand in for the shell: lay the tree out and
    // hand the metrics back, which is what the shell does every frame.
    let mut measure = |tc: &rux_layout::TextContent, _: Option<f32>| {
        (tc.text.chars().count() as f32 * 8.0, 16.0)
    };
    let layout = rux_layout::layout(&doc.root, 1000.0, 800.0, &mut measure);
    doc.set_metrics(layout.metrics);

    assert!(doc.apply_handler("measure()"));
    assert!(
        said(&doc, "the wide card is 220 x 64"),
        "the wide card's own box, not the plain one's: {:?}",
        texts(&doc.root)
    );

    // Focus reaches the input, and blur gives it up.
    doc.apply_handler("focus_note()");
    let asked = doc.take_focus_request().expect("focus() asked for focus");
    assert_eq!(asked.expect("for an element, not a blur").model, "note");
    doc.apply_handler("blur()");
    assert_eq!(doc.take_focus_request(), Some(None), "and blur asked to drop it");

    // Scrolling is the shell's to apply, so the document queues it.
    doc.apply_handler("reveal_end()");
    assert_eq!(doc.take_reveals().len(), 1, "one element asked to be revealed");
}

/// Drive the chart example: the line really is built from the readings, and it
/// really is rebuilt when they change.
///
/// This is the test standing in for a person looking at the window. `rux check`
/// never runs a script, so a `d` expression that returned nonsense would check
/// clean and draw nothing, and "the example loads" would still be true.
#[test]
fn the_chart_example_draws_its_readings() {
    fn paths(node: &rux_layout::Node, out: &mut Vec<rux_layout::PathContent>) {
        if let Some(p) = &node.path {
            out.push(p.clone());
        }
        for child in &node.children {
            paths(child, out);
        }
    }
    let all = |doc: &Document| {
        let mut v = Vec::new();
        paths(&doc.root, &mut v);
        v
    };

    let mut doc = Document::load(examples_dir().join("chart.rux")).expect("loads");
    let before = all(&doc);
    assert_eq!(before.len(), 2, "the band and the line");
    for p in &before {
        assert!(
            !p.commands.is_empty(),
            "a path with no geometry is an empty box: d was {:?}",
            p.d
        );
    }
    // Seven readings: a move and six lines, each line one cubic.
    let line = before.iter().find(|p| !p.d.contains('Z')).expect("the open line");
    assert_eq!(line.commands.len(), 7, "one command per reading: {:?}", line.d);

    // Add one, and the geometry follows.
    assert!(doc.apply_handler("add()"), "the tap changed state");
    let after = all(&doc);
    let line2 = after.iter().find(|p| !p.d.contains('Z')).expect("the open line");
    assert_eq!(line2.commands.len(), 8, "the new reading is drawn: {:?}", line2.d);
    assert_ne!(line.d, line2.d, "and the geometry actually changed");

    // Jolt moves every reading without changing how many there are, which is
    // the case `transition: d` exists for: same sequence, so it interpolates.
    let jolted = {
        assert!(doc.apply_handler("jolt()"));
        all(&doc)
    };
    let line3 = jolted.iter().find(|p| !p.d.contains('Z')).expect("the open line");
    assert_eq!(line3.commands.len(), 8, "the count held");
    assert_ne!(line2.d, line3.d, "and the values moved");
    assert!(
        rux_layout::path::lerp(&line2.commands, &line3.commands, 0.5).is_some(),
        "so the two interpolate, which is what makes the redraw walk"
    );
}

/// Drive the morph example: the three shapes really do share a command
/// sequence, so each one really can become the next.
///
/// The example's whole claim is that a square and a circle interpolate. If a
/// shape were ever edited into a different number of commands the morph would
/// silently become a cut, which is exactly the kind of rot nobody notices.
#[test]
fn the_morph_example_shapes_interpolate() {
    fn paths(node: &rux_layout::Node, out: &mut Vec<rux_layout::PathContent>) {
        if let Some(p) = &node.path {
            out.push(p.clone());
        }
        for child in &node.children {
            paths(child, out);
        }
    }

    let mut doc = Document::load(examples_dir().join("morph.rux")).expect("loads");
    let mut shapes = Vec::new();
    paths(&doc.root, &mut shapes);
    // The big one, plus the strip of three below it.
    assert_eq!(shapes.len(), 4, "the shape and the three glyphs");
    // A move, four curves and a close, for every one of them.
    for s in &shapes {
        assert_eq!(s.commands.len(), 6, "six commands: {:?}", s.d);
    }
    for pair in shapes.windows(2) {
        assert!(
            rux_layout::path::lerp(&pair[0].commands, &pair[1].commands, 0.5).is_some(),
            "every shape interpolates against every other, or the morph is a cut"
        );
    }

    // Walking on really does change which shape is drawn.
    let first = shapes[0].d.clone();
    assert!(doc.apply_handler("at = (at + 1) % shapes.length"));
    let mut next = Vec::new();
    paths(&doc.root, &mut next);
    assert_ne!(first, next[0].d, "the tap swapped the shape");
    assert_eq!(next[0].commands.len(), 6, "and the new one still parses");
}


/// The recipes are the pages that tell someone how to build a thing, so their
/// code has to be code that works. Each one is driven the way its page says to
/// drive it.
///
/// `/learn` has had this since it shipped and for the same reason: a tutorial
/// whose examples have quietly stopped working is worse than no tutorial, since
/// the reader assumes the mistake is theirs.
mod recipes {
    use super::*;

    fn recipe(name: &str) -> Document {
        Document::load(examples_dir().join("recipes").join(name))
            .unwrap_or_else(|e| panic!("{name} loads: {e:?}"))
    }

    fn texts(node: &rux_layout::Node) -> Vec<String> {
        let mut out: Vec<String> = node.text.iter().map(|t| t.text.clone()).collect();
        for child in &node.children {
            out.extend(texts(child));
        }
        out
    }

    fn shows(doc: &Document, needle: &str) -> bool {
        texts(&doc.root).iter().any(|t| t.contains(needle))
    }

    /// Sending adds a row, clears the field, and asks to be scrolled to.
    ///
    /// The reveal is the half worth asserting: the recipe's whole claim is that
    /// the thread follows its newest message, and the request is the only part
    /// of that a document can see. Where it *lands* is the shell's, and is what
    /// `rux_layout::containing_scroller` covers.
    #[test]
    fn the_message_list_sends_and_asks_to_be_revealed() {
        let mut doc = recipe("message-list.rux");
        assert!(doc.apply_handler("draft = \"a new one\""), "typed");
        assert!(doc.apply_handler("send()"), "sent");
        assert!(shows(&doc, "a new one"), "the message is on screen");
        assert_eq!(
            doc.take_reveals().len(),
            1,
            "and the list asked to be scrolled to its anchor"
        );
    }

    /// An empty draft sends nothing, which is the guard every composer needs and
    /// the one most likely to be left out.
    #[test]
    fn the_message_list_will_not_send_nothing() {
        let mut doc = recipe("message-list.rux");
        let before = texts(&doc.root).len();
        assert!(doc.apply_handler("draft = \"   \""), "whitespace only");
        // `apply_handler` reports whether anything moved, and nothing did: the
        // guard returns before the push, so this is the assertion rather than a
        // thing to unwrap.
        assert!(!doc.apply_handler("send()"), "sending nothing changes nothing");
        assert_eq!(texts(&doc.root).len(), before, "and added no row");
    }

    /// Navigating holds the outgoing page beside the incoming one, which is the
    /// behaviour the recipe's `position: absolute` line exists to cope with.
    #[test]
    fn the_tab_bar_holds_both_pages_during_a_swap() {
        let mut doc = recipe("tab-bar.rux");
        assert!(shows(&doc, "inbox"), "starts on the index route");
        assert!(doc.apply_handler("navigate(\"/drafts\")"), "navigated");
        assert!(shows(&doc, "inbox"), "the page being left is still built");
        assert!(shows(&doc, "drafts"), "and the one arriving is too");
        // Past the 240ms the style declares, the swap commits and it is gone.
        let _ = doc.advance_swaps(0.0);
        let _ = doc.advance_swaps(400.0);
        assert!(!shows(&doc, "not inside the router"), "the outgoing page has gone");
    }

    /// The modal opens, the scrim dismisses, and Cancel and Delete are told
    /// apart. The swallow on the dialog is a hit-test fact and so belongs to the
    /// window, not here.
    #[test]
    fn the_modal_opens_and_both_answers_close_it() {
        let mut doc = recipe("modal.rux");
        assert!(!shows(&doc, "Delete the archive?"), "closed to begin with");

        assert!(doc.apply_handler("open = true"), "opened");
        assert!(shows(&doc, "Delete the archive?"), "the dialog is on screen");

        assert!(doc.apply_handler("dismiss()"), "dismissed");
        assert!(shows(&doc, "left alone"), "and said so");

        assert!(doc.apply_handler("open = true"), "opened again");
        assert!(doc.apply_handler("confirm()"), "confirmed");
        assert!(shows(&doc, "archive deleted"), "the other answer is different");
    }
}
