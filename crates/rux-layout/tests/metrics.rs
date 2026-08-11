//! Per-node metrics: where each laid-out node ended up, keyed by its tree path.
//!
//! This is what `query()` reads back when a handler asks where something is, so
//! the paths have to name the nodes the element index names, and the boxes have
//! to be the boxes actually on screen.

use rux_layout::*;

fn boxed(style: Style, children: Vec<Node>) -> Node {
    let mut n = Node::new(style);
    n.children = children;
    n
}

fn flex(axis: Axis) -> Style {
    Style { display: Display::Flex, axis, ..Default::default() }
}

fn sized(w: f32, h: f32) -> Style {
    Style { width: Some(Len::Px(w)), height: Some(Len::Px(h)), ..Default::default() }
}

fn metrics(root: Node) -> Vec<NodeMetrics> {
    let mut measure = |_: &rux_layout::TextContent, _: Option<f32>| (0.0, 0.0);
    layout(&root, 1000.0, 800.0, &mut measure).metrics
}

fn at<'a>(m: &'a [NodeMetrics], path: &[usize]) -> &'a NodeMetrics {
    m.iter().find(|e| e.path == path).unwrap_or_else(|| panic!("no metrics at {path:?}"))
}

/// The root is the empty path, and each child is its index under its parent.
/// This is the same identity the element index and the binding registry use; if
/// it drifts, a query resolves to somebody else's box.
#[test]
fn paths_name_nodes_by_child_index() {
    let m = metrics(boxed(
        flex(Axis::Column),
        vec![boxed(sized(100.0, 50.0), vec![]), boxed(sized(200.0, 60.0), vec![])],
    ));

    assert!(m.iter().any(|e| e.path.is_empty()), "the root is the empty path");
    assert_eq!(at(&m, &[0]).width, 100.0);
    assert_eq!(at(&m, &[1]).width, 200.0);
}

/// Nesting composes, and a path is the full chain from the root rather than an
/// index within one parent.
#[test]
fn nested_paths_are_the_whole_chain() {
    let m = metrics(boxed(
        flex(Axis::Column),
        vec![boxed(flex(Axis::Column), vec![boxed(sized(80.0, 40.0), vec![])])],
    ));

    let inner = at(&m, &[0, 0]);
    assert_eq!((inner.width, inner.height), (80.0, 40.0));
}

/// Boxes are absolute window pixels, which is what "where is this" means to
/// whoever asked. A column stacks, so the second child starts below the first.
#[test]
fn geometry_is_absolute_and_stacks_down_a_column() {
    let m = metrics(boxed(
        flex(Axis::Column),
        vec![boxed(sized(100.0, 50.0), vec![]), boxed(sized(100.0, 60.0), vec![])],
    ));

    assert_eq!(at(&m, &[0]).y, 0.0);
    assert_eq!(at(&m, &[1]).y, 50.0, "the second sits under the first");
    assert_eq!(at(&m, &[1]).height, 60.0);
}

/// A node hidden by `r-show="false"` keeps its layout slot but is not on
/// screen, and geometry is a property of what is shown. It reports nothing, so
/// an absent answer stays distinguishable from a zero-sized one.
#[test]
fn a_hidden_node_reports_no_metrics() {
    let mut hidden = boxed(sized(100.0, 50.0), vec![]);
    hidden.hidden = true;
    let m = metrics(boxed(flex(Axis::Column), vec![hidden, boxed(sized(100.0, 60.0), vec![])]));

    assert!(!m.iter().any(|e| e.path == [0]), "the hidden node has no box to report");
    assert_eq!(at(&m, &[1]).height, 60.0, "but its sibling still does");
}

/// Every visible node gets an entry, not only the ones that happen to be
/// tappable or hover-styled. Those were the only nodes carrying a path before
/// this existed, and a query can ask about any of them.
#[test]
fn every_visible_node_is_measured_not_just_interactive_ones() {
    let m = metrics(boxed(
        flex(Axis::Column),
        vec![boxed(sized(10.0, 10.0), vec![]), boxed(sized(20.0, 20.0), vec![])],
    ));

    // Root plus both children, none of which has a handler or a hover rule.
    assert_eq!(m.len(), 3, "{m:?}");
}
