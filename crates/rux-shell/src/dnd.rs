//! Dragging something from one box and putting it on another.
//!
//! Rux supplies the parts an author cannot build: which element a press picks
//! up, which target the pointer is over, and which slot among that target's
//! items it is at, and the events that say so. What the drag looks like (the
//! copy that follows the pointer, a gap where the thing was, a row sliding
//! aside) and what a drop does to the data are the author's.
//!
//! This file is in two halves. The first is geometry and nothing else: given
//! the elements that declared `drag-data`, `drag-handle`, `@drop` and the rest,
//! and where each is, it answers the three questions. The second is the state
//! of a press that may turn into a drag, and what it fires.

use rux_layout::{Dnd, Node};

/// `(x, y, width, height)` in the window's logical pixels.
pub type Rect = (f32, f32, f32, f32);

/// An element that said something about dragging and dropping, and where it is
/// in the tree.
#[derive(Clone, Debug, PartialEq)]
pub struct Spot {
    pub path: Vec<usize>,
    pub dnd: Dnd,
    pub instance: Option<String>,
}

/// The tree's spots, in document order.
pub fn spots(root: &Node) -> Vec<Spot> {
    fn walk(node: &Node, path: &mut Vec<usize>, out: &mut Vec<Spot>) {
        if let Some(dnd) = node.dnd() {
            out.push(Spot { path: path.clone(), dnd: dnd.clone(), instance: node.instance.clone() });
        }
        for (i, child) in node.children.iter().enumerate() {
            path.push(i);
            walk(child, path, out);
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(root, &mut Vec::new(), &mut out);
    out
}

/// A spot and its box.
#[derive(Clone, Copy, Debug)]
pub struct Placed<'a> {
    pub spot: &'a Spot,
    pub rect: Rect,
}

fn contains(r: Rect, x: f32, y: f32) -> bool {
    x >= r.0 && x <= r.0 + r.2 && y >= r.1 && y <= r.1 + r.3
}

/// The deepest source whose path is `path` or an ancestor of it.
fn nearest_source(all: &[Placed], path: &[usize]) -> Option<usize> {
    all.iter()
        .enumerate()
        .filter(|(_, s)| s.spot.dnd.is_source() && path.starts_with(&s.spot.path))
        .max_by_key(|(_, s)| s.spot.path.len())
        .map(|(i, _)| i)
}

/// What a press at a point picks up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pick {
    /// The source, as an index into the placed spots.
    pub source: usize,
    /// Whether it has a handle, and so is picked up by a handle only. A finger
    /// can then claim the drag at once, where a source with no handle has to
    /// wait out a long press so a scroll still scrolls.
    pub handle: bool,
}

/// The source a press at `(x, y)` picks up: the deepest source under the point,
/// and, if it has handles, only when the point is on one. A press on a nested
/// source's own box never falls through to the source around it.
pub fn source_at(all: &[Placed], x: f32, y: f32) -> Option<Pick> {
    let source = all
        .iter()
        .enumerate()
        .filter(|(_, s)| s.spot.dnd.is_source() && contains(s.rect, x, y))
        .max_by_key(|(_, s)| s.spot.path.len())?
        .0;
    let own = &all[source].spot.path;
    let handles: Vec<&Placed> = all
        .iter()
        .filter(|h| h.spot.dnd.handle && h.spot.path.starts_with(own) && nearest_source(all, &h.spot.path) == Some(source))
        .collect();
    if handles.is_empty() {
        Some(Pick { source, handle: false })
    } else if handles.iter().any(|h| contains(h.rect, x, y)) {
        Some(Pick { source, handle: true })
    } else {
        None
    }
}

/// The paths of every source carrying `data`.
fn carrying<'a>(all: &'a [Placed], data: &str) -> Vec<&'a [usize]> {
    all.iter()
        .filter(|s| s.spot.dnd.data.as_deref() == Some(data))
        .map(|s| &s.spot.path[..])
        .collect()
}

/// The target that takes the thing being dragged at `(x, y)`: the deepest one
/// under the point that accepts its group, and is not inside the thing itself,
/// so a section cannot be dropped into itself. `data` is found again by value
/// because a live move has rebuilt the list the source was in.
pub fn target_at(all: &[Placed], x: f32, y: f32, group: Option<&str>, data: &str) -> Option<usize> {
    let inside = carrying(all, data);
    all.iter()
        .enumerate()
        .filter(|(_, t)| {
            t.spot.dnd.is_target()
                && t.spot.dnd.accepts.as_deref() == group
                && contains(t.rect, x, y)
                && !inside.iter().any(|d| t.spot.path.starts_with(d))
        })
        .max_by_key(|(_, t)| t.spot.path.len())
        .map(|(i, _)| i)
}

/// Where among a target's items the thing being dragged would go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    /// Counted without the dragged item, so splicing at it is right.
    pub index: usize,
    /// The `drag-data` of the item it would land in front of; `None` at the end.
    pub before: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Flow {
    Column,
    Row,
    Wrap,
}

/// Which way a run of boxes is laid out, read off where they are rather than
/// off the container's CSS, which could be a grid, a wrapping row or anything.
fn flow(items: &[&Placed]) -> Flow {
    let Some(first) = items.first().map(|i| i.rect) else { return Flow::Column };
    let across = |r: Rect| first.0 < r.0 + r.2 && r.0 < first.0 + first.2;
    let down = |r: Rect| first.1 < r.1 + r.3 && r.1 < first.1 + first.3;
    // Every item over the first is a column, every one beside it is a row,
    // and a mixture is a wrapping run (or a grid).
    if items.iter().all(|i| across(i.rect)) {
        Flow::Column
    } else if items.iter().all(|i| down(i.rect)) {
        Flow::Row
    } else {
        Flow::Wrap
    }
}

/// The slot at `(x, y)` among the items of `target`: the sources whose nearest
/// target is it, the dragged one left out. A column is read by the pointer's
/// height against each item's middle, a row by its width, and anything that
/// wraps in reading order.
pub fn slot(all: &[Placed], target: usize, data: &str, x: f32, y: f32) -> Slot {
    let t = &all[target].spot.path;
    let inside = carrying(all, data);
    let items: Vec<&Placed> = all
        .iter()
        .filter(|s| {
            s.spot.dnd.is_source()
                && s.spot.dnd.data.as_deref() != Some(data)
                && s.spot.path.len() > t.len()
                && s.spot.path.starts_with(t)
                && !inside.iter().any(|d| s.spot.path.starts_with(d))
                // Its nearest target is this one: no other target between.
                && !all.iter().any(|o| {
                    o.spot.dnd.is_target()
                        && o.spot.path.len() > t.len()
                        && o.spot.path.len() < s.spot.path.len()
                        && s.spot.path.starts_with(&o.spot.path)
                })
        })
        .collect();
    let way = flow(&items);
    let index = items
        .iter()
        .filter(|s| {
            let r = s.rect;
            match way {
                Flow::Column => y > r.1 + r.3 / 2.0,
                Flow::Row => x > r.0 + r.2 / 2.0,
                Flow::Wrap => y > r.1 + r.3 || (y >= r.1 && x > r.0 + r.2 / 2.0),
            }
        })
        .count();
    Slot { index, before: items.get(index).and_then(|s| s.spot.dnd.data.clone()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dnd(data: Option<&str>) -> Dnd {
        Dnd { data: data.map(str::to_string), ..Default::default() }
    }

    fn spot(path: &[usize], dnd: Dnd) -> Spot {
        Spot { path: path.to_vec(), dnd, instance: None }
    }

    fn source(path: &[usize], data: &str) -> Spot {
        spot(path, dnd(Some(data)))
    }

    fn target(path: &[usize]) -> Spot {
        spot(path, Dnd { accepts: Some("g".into()), ..Default::default() })
    }

    fn placed<'a>(spots: &'a [Spot], rects: &[Rect]) -> Vec<Placed<'a>> {
        spots.iter().zip(rects).map(|(spot, rect)| Placed { spot, rect: *rect }).collect()
    }

    /// A list of three rows, 100 wide and 20 tall, from y = 0.
    fn column() -> (Vec<Spot>, Vec<Rect>) {
        let spots = vec![
            target(&[0]),
            source(&[0, 0], "a"),
            source(&[0, 1], "b"),
            source(&[0, 2], "c"),
        ];
        let rects = vec![(0.0, 0.0, 100.0, 60.0), (0.0, 0.0, 100.0, 20.0), (0.0, 20.0, 100.0, 20.0), (0.0, 40.0, 100.0, 20.0)];
        (spots, rects)
    }

    #[test]
    fn a_press_picks_up_the_source_under_it() {
        let (s, r) = column();
        let all = placed(&s, &r);
        assert_eq!(source_at(&all, 10.0, 25.0), Some(Pick { source: 2, handle: false }));
        assert_eq!(source_at(&all, 500.0, 25.0), None);
    }

    #[test]
    fn a_source_with_a_handle_is_picked_up_by_the_handle_only() {
        let mut s = vec![source(&[0], "a"), spot(&[0, 0], Dnd { handle: true, ..Default::default() })];
        let r = vec![(0.0, 0.0, 100.0, 20.0), (0.0, 0.0, 20.0, 20.0)];
        let all = placed(&s, &r);
        assert_eq!(source_at(&all, 5.0, 5.0), Some(Pick { source: 0, handle: true }));
        assert_eq!(source_at(&all, 60.0, 5.0), None, "the body is not the handle");
        s[0].dnd.handle = false;
    }

    #[test]
    fn a_nested_source_is_not_the_one_around_it() {
        // A section holding a lesson: a press on the lesson picks the lesson.
        let s = vec![source(&[0], "section"), source(&[0, 1], "lesson")];
        let r = vec![(0.0, 0.0, 100.0, 100.0), (0.0, 30.0, 100.0, 20.0)];
        let all = placed(&s, &r);
        assert_eq!(source_at(&all, 10.0, 40.0).map(|p| p.source), Some(1));
        assert_eq!(source_at(&all, 10.0, 80.0).map(|p| p.source), Some(0));
    }

    #[test]
    fn the_slot_in_a_column_is_read_against_each_middle_and_leaves_the_dragged_out() {
        let (s, r) = column();
        let all = placed(&s, &r);
        // Dragging "b": the items are a and c, at 0..20 and 40..60.
        assert_eq!(slot(&all, 0, "b", 50.0, 5.0), Slot { index: 0, before: Some("a".into()) });
        assert_eq!(slot(&all, 0, "b", 50.0, 25.0), Slot { index: 1, before: Some("c".into()) });
        assert_eq!(slot(&all, 0, "b", 50.0, 58.0), Slot { index: 2, before: None });
    }

    #[test]
    fn the_slot_in_a_row_is_read_by_width() {
        let s = vec![target(&[0]), source(&[0, 0], "a"), source(&[0, 1], "b")];
        let r = vec![(0.0, 0.0, 200.0, 20.0), (0.0, 0.0, 100.0, 20.0), (100.0, 0.0, 100.0, 20.0)];
        let all = placed(&s, &r);
        assert_eq!(slot(&all, 0, "z", 20.0, 10.0).index, 0);
        assert_eq!(slot(&all, 0, "z", 120.0, 10.0).index, 1);
        assert_eq!(slot(&all, 0, "z", 190.0, 10.0).index, 2);
    }

    #[test]
    fn a_wrapping_run_is_read_in_reading_order() {
        // Two across, two below.
        let s = vec![
            target(&[0]),
            source(&[0, 0], "a"),
            source(&[0, 1], "b"),
            source(&[0, 2], "c"),
            source(&[0, 3], "d"),
        ];
        let r = vec![
            (0.0, 0.0, 200.0, 60.0),
            (0.0, 0.0, 100.0, 20.0),
            (100.0, 0.0, 100.0, 20.0),
            (0.0, 30.0, 100.0, 20.0),
            (100.0, 30.0, 100.0, 20.0),
        ];
        let all = placed(&s, &r);
        assert_eq!(slot(&all, 0, "z", 150.0, 10.0).index, 1, "right half of the first row, past a only");
        assert_eq!(slot(&all, 0, "z", 20.0, 40.0).index, 2, "left of c");
        assert_eq!(slot(&all, 0, "z", 120.0, 40.0).index, 3, "left of d");
        assert_eq!(slot(&all, 0, "z", 180.0, 40.0).index, 4, "past d: the end");
    }

    #[test]
    fn an_empty_target_takes_the_first_slot() {
        let s = vec![target(&[0])];
        let all = placed(&s, &[(0.0, 0.0, 100.0, 40.0)]);
        assert_eq!(slot(&all, 0, "z", 10.0, 10.0), Slot { index: 0, before: None });
    }

    #[test]
    fn the_deepest_target_that_accepts_the_group_wins() {
        // An outline holding a section's lesson list.
        let s = vec![target(&[0]), source(&[0, 0], "sec"), target(&[0, 0, 1]), source(&[0, 0, 1, 0], "l1")];
        let r = vec![(0.0, 0.0, 100.0, 200.0), (0.0, 0.0, 100.0, 100.0), (0.0, 30.0, 100.0, 70.0), (0.0, 30.0, 100.0, 20.0)];
        let all = placed(&s, &r);
        assert_eq!(target_at(&all, 10.0, 40.0, Some("g"), "x"), Some(2));
        assert_eq!(target_at(&all, 10.0, 150.0, Some("g"), "x"), Some(0));
        assert_eq!(target_at(&all, 10.0, 40.0, Some("other"), "x"), None, "another group");
        assert_eq!(target_at(&all, 10.0, 40.0, None, "x"), None, "no group takes only a target with none");
    }

    #[test]
    fn a_section_cannot_be_dropped_into_itself() {
        let s = vec![target(&[0]), source(&[0, 0], "sec"), target(&[0, 0, 1])];
        let r = vec![(0.0, 0.0, 100.0, 200.0), (0.0, 0.0, 100.0, 100.0), (0.0, 30.0, 100.0, 70.0)];
        let all = placed(&s, &r);
        assert_eq!(target_at(&all, 10.0, 40.0, Some("g"), "sec"), Some(0), "its own list is skipped");
    }

    #[test]
    fn the_items_of_a_target_are_those_it_is_nearest_to() {
        // The outline's items are the sections, not the lessons inside one.
        let s = vec![target(&[0]), source(&[0, 0], "sec"), target(&[0, 0, 1]), source(&[0, 0, 1, 0], "l1")];
        let r = vec![(0.0, 0.0, 100.0, 200.0), (0.0, 0.0, 100.0, 100.0), (0.0, 30.0, 100.0, 70.0), (0.0, 30.0, 100.0, 20.0)];
        let all = placed(&s, &r);
        assert_eq!(slot(&all, 0, "x", 10.0, 190.0), Slot { index: 1, before: None });
        assert_eq!(slot(&all, 2, "x", 10.0, 90.0), Slot { index: 1, before: None });
    }
}
