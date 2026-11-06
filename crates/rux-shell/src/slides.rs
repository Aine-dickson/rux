//! Keyed rows that slide to their new place.
//!
//! A row of a keyed `r-for` whose `transition` names `transform` (not `all`,
//! which would make every row beside an animating box slide too)
//! is drawn sliding from where it was to where the list has put it, when the
//! tree changes and it lands somewhere else. Nothing about a list is special:
//! a row moved by a button, by a sort, by a filter or by a drag is moved the
//! same way, because the only thing that decides is the row's key and its
//! `transition`.
//!
//! The layout does not know where a row was a frame ago, so the memory is
//! here. The slide is drawn through [`rux_layout::LayoutState::set_nudges`]:
//! the row is laid out in its new place and drawn `(dx, dy)` away from it,
//! with the offset eased to zero.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use rux_layout::{AnimProp, Easing, Node};

/// A keyed row that has asked to slide: where it is in the tree, who it is,
/// and the `transition` that says how.
#[derive(Clone, Debug, PartialEq)]
pub struct MoveRow {
    pub path: Vec<usize>,
    pub key: String,
    pub duration: f32,
    pub delay: f32,
    pub easing: Easing,
}

/// The rows of a tree that slide. A key that appears twice is left out: with
/// two rows claiming one identity there is no telling which moved where.
pub fn move_rows(root: &Node) -> Vec<MoveRow> {
    fn walk(node: &Node, path: &mut Vec<usize>, out: &mut Vec<MoveRow>) {
        if let Some(key) = &node.key {
            let t = node
                .style
                .transitions
                .iter()
                .find(|t| t.property == AnimProp::Transform && t.duration > 0.0);
            if let Some(t) = t {
                out.push(MoveRow {
                    path: path.clone(),
                    key: key.clone(),
                    duration: t.duration,
                    delay: t.delay,
                    easing: t.easing,
                });
            }
        }
        for (i, child) in node.children.iter().enumerate() {
            path.push(i);
            walk(child, path, out);
            path.pop();
        }
    }
    let mut rows = Vec::new();
    walk(root, &mut Vec::new(), &mut rows);
    let mut seen = HashSet::new();
    let mut twice = HashSet::new();
    for r in &rows {
        if !seen.insert(r.key.clone()) {
            twice.insert(r.key.clone());
        }
    }
    rows.retain(|r| !twice.contains(&r.key));
    rows
}

/// One row on its way: how far from its place it started, and when.
#[derive(Clone, Debug)]
struct Slide {
    from: (f32, f32),
    began: Instant,
    duration: f32,
    delay: f32,
    easing: Easing,
}

impl Slide {
    /// The offset from its place at `now`, `None` once it has arrived.
    fn at(&self, now: Instant) -> Option<(f32, f32)> {
        let ms = now.saturating_duration_since(self.began).as_secs_f32() * 1000.0 - self.delay;
        if ms >= self.duration {
            return None;
        }
        let left = 1.0 - self.easing.eval((ms / self.duration).clamp(0.0, 1.0));
        Some((self.from.0 * left, self.from.1 * left))
    }
}

#[derive(Default)]
pub struct Slides {
    revision: Option<u64>,
    rows: Vec<MoveRow>,
    /// Where each row was laid out last frame, by key, in the content's own
    /// coordinates (the scroll offset added back), so scrolling is not a move.
    placed: HashMap<String, (f32, f32)>,
    active: HashMap<String, Slide>,
    /// What was last handed to the layout.
    applied: HashMap<Vec<usize>, (f32, f32)>,
}

impl Slides {
    /// Whether any row is on its way, so the next frame has to be drawn.
    pub fn animating(&self) -> bool {
        !self.active.is_empty()
    }

    /// Whether the tree has any row that slides at all. When it has none, a
    /// frame has nothing to do here.
    pub fn idle(&self) -> bool {
        self.rows.is_empty() && self.active.is_empty() && self.applied.is_empty()
    }

    /// Read the tree's sliding rows, if it is not the tree seen last. Returns
    /// whether it changed.
    pub fn prepare(&mut self, root: &Node, revision: u64) -> bool {
        if self.revision == Some(revision) {
            return false;
        }
        self.revision = Some(revision);
        self.rows = move_rows(root);
        true
    }

    /// The paths of the rows to look for.
    pub fn paths(&self) -> impl Iterator<Item = &[usize]> {
        self.rows.iter().map(|r| &r.path[..])
    }

    fn nudges(&self, now: Instant) -> HashMap<Vec<usize>, (f32, f32)> {
        self.rows
            .iter()
            .filter_map(|r| Some((r.path.clone(), self.active.get(&r.key)?.at(now)?)))
            .filter(|(_, d)| d.0 != 0.0 || d.1 != 0.0)
            .collect()
    }

    /// What the layout is to draw the rows away by for the frame at `now`, or
    /// `None` when that is what it was last told.
    pub fn changed_nudges(&mut self, now: Instant) -> Option<HashMap<Vec<usize>, (f32, f32)>> {
        let now_nudges = self.nudges(now);
        if now_nudges == self.applied {
            return None;
        }
        self.applied = now_nudges.clone();
        Some(now_nudges)
    }

    /// Look at where the layout put the rows. `drawn` is each row's drawn
    /// place, by path, in content coordinates; a row the layout did not draw
    /// is left out. `moved` says the tree changed since the last frame, the
    /// only time a row that is somewhere else has been moved: a window
    /// resized under a list is not the list moving.
    pub fn observe(&mut self, drawn: &HashMap<Vec<usize>, (f32, f32)>, moved: bool, now: Instant) {
        self.active.retain(|_, s| s.at(now).is_some());
        let mut here = HashMap::with_capacity(self.rows.len());
        for row in &self.rows {
            let Some(&seen) = drawn.get(&row.path) else { continue };
            // Where the list put it: what was drawn, less what it was nudged.
            let nudge = self.applied.get(&row.path).copied().unwrap_or((0.0, 0.0));
            let placed = (seen.0 - nudge.0, seen.1 - nudge.1);
            if moved {
                if let Some(&before) = self.placed.get(&row.key) {
                    if (before.0 - placed.0).abs() > 0.5 || (before.1 - placed.1).abs() > 0.5 {
                        // From where it was being drawn, so a row moved again
                        // mid-slide carries on from where it appeared.
                        let was = self.active.get(&row.key).and_then(|s| s.at(now)).unwrap_or((0.0, 0.0));
                        let from = (before.0 + was.0 - placed.0, before.1 + was.1 - placed.1);
                        self.active.insert(
                            row.key.clone(),
                            Slide { from, began: now, duration: row.duration, delay: row.delay, easing: row.easing },
                        );
                    }
                }
            }
            here.insert(row.key.clone(), placed);
        }
        self.placed = here;
    }

    /// When the next frame is wanted while rows are on their way.
    pub const FRAME: Duration = Duration::from_millis(16);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(path: &[usize], key: &str) -> MoveRow {
        MoveRow { path: path.to_vec(), key: key.into(), duration: 100.0, delay: 0.0, easing: Easing::Linear }
    }

    fn slides(rows: Vec<MoveRow>) -> Slides {
        Slides { rows, revision: Some(1), ..Default::default() }
    }

    fn at(x: f32, y: f32) -> (f32, f32) {
        (x, y)
    }

    #[test]
    fn a_row_that_lands_elsewhere_slides_from_where_it_was() {
        let t0 = Instant::now();
        let mut s = slides(vec![row(&[0], "a"), row(&[1], "b")]);
        let first: HashMap<_, _> = [(vec![0], at(0.0, 0.0)), (vec![1], at(0.0, 40.0))].into();
        s.observe(&first, true, t0);
        assert!(!s.animating(), "the first sight is not a move");
        // The list swaps them: a is at 40, b at 0.
        s.rows = vec![row(&[1], "a"), row(&[0], "b")];
        let swapped: HashMap<_, _> = [(vec![0], at(0.0, 0.0)), (vec![1], at(0.0, 40.0))].into();
        s.observe(&swapped, true, t0);
        assert!(s.animating());
        let n = s.changed_nudges(t0).unwrap();
        assert_eq!(n[&vec![1]], at(0.0, -40.0), "a starts where it was");
        assert_eq!(n[&vec![0]], at(0.0, 40.0), "b too");
        let mid = s.changed_nudges(t0 + Duration::from_millis(50)).unwrap();
        assert!((mid[&vec![1]].1 + 20.0).abs() < 0.01, "halfway at half the time");
        s.observe(&swapped, false, t0 + Duration::from_millis(150));
        assert!(!s.animating(), "arrived");
        assert_eq!(s.changed_nudges(t0 + Duration::from_millis(150)), Some(HashMap::new()));
    }

    #[test]
    fn a_resize_is_not_a_move() {
        let t0 = Instant::now();
        let mut s = slides(vec![row(&[0], "a")]);
        s.observe(&[(vec![0], at(0.0, 0.0))].into(), true, t0);
        s.observe(&[(vec![0], at(0.0, 90.0))].into(), false, t0);
        assert!(!s.animating());
    }

    #[test]
    fn moved_again_mid_slide_carries_on_from_where_it_is_drawn() {
        let t0 = Instant::now();
        let mut s = slides(vec![row(&[0], "a")]);
        s.observe(&[(vec![0], at(0.0, 0.0))].into(), true, t0);
        s.observe(&[(vec![0], at(0.0, 100.0))].into(), true, t0);
        let _ = s.changed_nudges(t0);
        // Half way through, it is laid out at 100 and drawn at 50. The list
        // then puts it at 0, so with the nudge of -50 still applied the layout
        // draws it at -50.
        let half = t0 + Duration::from_millis(50);
        let _ = s.changed_nudges(half);
        s.observe(&[(vec![0], at(0.0, -50.0))].into(), true, half);
        let n = s.changed_nudges(half).unwrap();
        assert!((n[&vec![0]].1 - 50.0).abs() < 0.01, "still drawn at 50: {n:?}");
    }
}
