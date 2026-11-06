//! The press that may become a drag, and what a drag fires.
//!
//! The questions (what a press picks up, which target the pointer is over,
//! which slot) are answered by the geometry in [`crate::dnd`]. This is the
//! state around them: a press that has not moved far enough, a drag in hand,
//! and the events each step runs.

use std::collections::{HashMap, HashSet};

use rux_layout::DragEvent;
use rux_reactive::Value;

use crate::dnd::{slot, source_at, spots, target_at, Placed, Rect, Slot, Spot};
use crate::{App, TAP_SLOP};

/// A press on a source that has not yet moved far enough to be a drag.
#[derive(Clone, Debug)]
struct Pending {
    source: Spot,
    rect: Rect,
    /// A handle was what was pressed.
    handle: bool,
    start: (f32, f32),
    from_touch: bool,
}

/// The target the dragged thing is over, and which slot.
#[derive(Clone, Debug)]
struct Over {
    spot: Spot,
    rect: Rect,
    slot: Slot,
}

#[derive(Clone, Debug)]
struct Session {
    source: Spot,
    rect: Rect,
    /// Where in the source the press landed.
    grab: (f32, f32),
    /// `drag-data` as the literal the layout carries, which is how the thing is
    /// found again, and as the value a handler is handed.
    literal: String,
    value: Value,
    group: Option<String>,
    over: Option<Over>,
}

/// What a drag has in hand besides the pointer: what `DropEvent` adds.
struct Fields<'a> {
    session: &'a Session,
    slot: Option<&'a Slot>,
    dropped: bool,
}

#[derive(Default)]
pub struct DragDrop {
    revision: Option<u64>,
    spots: Vec<Spot>,
    pending: Option<Pending>,
    session: Option<Session>,
}

impl App {
    /// Run `f` over the spots that are on screen, each with its box as the
    /// layout put it: where it is laid out, not where a slide has it for now,
    /// so a row on its way does not change which slot the pointer is at.
    fn with_placed<R>(&mut self, f: impl FnOnce(&[Placed]) -> R) -> R {
        let revision = self.document.root.revision();
        if self.drag_drop.revision != Some(revision) {
            self.drag_drop.spots = spots(&self.document.root);
            self.drag_drop.revision = Some(revision);
        }
        let all = &self.drag_drop.spots;
        let wanted: HashSet<&[usize]> = all.iter().map(|s| &s.path[..]).collect();
        let mut rects: HashMap<&[usize], Rect> = HashMap::new();
        for m in &self.metrics {
            if wanted.contains(&*m.path) {
                let (dx, dy) = self.slides.nudge_at(&m.path);
                rects.insert(&m.path, (m.x - dx, m.y - dy, m.width, m.height));
            }
        }
        let placed: Vec<Placed> =
            all.iter().filter_map(|s| Some(Placed { spot: s, rect: *rects.get(&s.path[..])? })).collect();
        f(&placed)
    }

    /// A press at `(fx, fy)`: remember the source it landed on, if any, and say
    /// where that is and whether a handle was pressed.
    pub(crate) fn dnd_press(&mut self, fx: f32, fy: f32, from_touch: bool) -> Option<(Rect, bool)> {
        self.drag_drop.pending = None;
        if self.drag_drop.session.is_some() {
            return None;
        }
        let (source, rect, handle) = self.with_placed(|all| {
            let pick = source_at(all, fx, fy)?;
            Some((all[pick.source].spot.clone(), all[pick.source].rect, pick.handle))
        })?;
        // A box drawn over the source (a menu, a backdrop) keeps the press from
        // it: the topmost region under the point has to be inside the source.
        if let Some(top) = rux_layout::hit_at(&self.hits, fx, fy) {
            let inside = top.x >= rect.0 - 1.0
                && top.y >= rect.1 - 1.0
                && top.x + top.width <= rect.0 + rect.2 + 1.0
                && top.y + top.height <= rect.1 + rect.3 + 1.0;
            if !inside {
                return None;
            }
        }
        self.drag_drop.pending = Some(Pending { source, rect, handle, start: (fx, fy), from_touch });
        Some((rect, handle))
    }

    /// The pointer moved. Whether a drag has it, so nothing else does.
    pub(crate) fn dnd_move(&mut self, fx: f32, fy: f32) -> bool {
        if self.drag_drop.session.is_some() {
            self.dnd_track(fx, fy);
            return true;
        }
        let Some(p) = &self.drag_drop.pending else { return false };
        if (fx - p.start.0).hypot(fy - p.start.1) <= TAP_SLOP as f32 {
            return false;
        }
        // A finger that moves before it has been held is scrolling, unless it
        // is on a handle, which is what says it is not.
        if p.from_touch && !p.handle {
            self.drag_drop.pending = None;
            return false;
        }
        self.dnd_begin();
        self.dnd_track(fx, fy);
        true
    }

    /// A press on a source with no handle has rested long enough, on a finger.
    pub(crate) fn dnd_long_press(&mut self) -> bool {
        let Some(p) = &self.drag_drop.pending else { return false };
        if !p.from_touch || p.handle || self.drag_drop.session.is_some() {
            return false;
        }
        let at = p.start;
        self.dnd_begin();
        self.dnd_track(at.0, at.1);
        true
    }

    /// Whether a finger resting on this press would pick something up, which is
    /// what makes the long press worth waiting for.
    pub(crate) fn dnd_waits_for_long_press(&self) -> bool {
        self.drag_drop.pending.as_ref().is_some_and(|p| p.from_touch && !p.handle)
    }

    /// The pending press becomes a drag.
    fn dnd_begin(&mut self) {
        let Some(p) = self.drag_drop.pending.take() else { return };
        let Some(literal) = p.source.dnd.data.clone() else { return };
        let value = self.document.eval_literal(&literal).unwrap_or(Value::Null);
        let session = Session {
            grab: (p.start.0 - p.rect.0, p.start.1 - p.rect.1),
            group: p.source.dnd.group.clone(),
            source: p.source,
            rect: p.rect,
            literal,
            value,
            over: None,
        };
        // The press is the drag's now: no `@drag`, `@swipe` or long press of
        // the element's own, and on a finger the page does not scroll under it.
        if let Some(g) = self.gesture.as_mut() {
            g.dragging = true;
            g.handlers.retain(|(k, _)| {
                !matches!(k, rux_layout::Gesture::Drag | rux_layout::Gesture::Swipe | rux_layout::Gesture::LongPress)
            });
        }
        self.gesture_deadline = None;
        self.drag_drop.session = Some(session.clone());
        self.dnd_interaction();
        self.fire_dnd(
            &session.source,
            session.rect,
            DragEvent::Start,
            p.start,
            Fields { session: &session, slot: None, dropped: false },
        );
    }

    /// The pointer is at `(fx, fy)` with something in hand: tell the source it
    /// moved, and the targets what it is over.
    fn dnd_track(&mut self, fx: f32, fy: f32) {
        let Some(session) = self.drag_drop.session.clone() else { return };
        self.fire_dnd(
            &session.source,
            session.rect,
            DragEvent::Move,
            (fx, fy),
            Fields { session: &session, slot: None, dropped: false },
        );
        let found = self.with_placed(|all| {
            let t = target_at(all, fx, fy, session.group.as_deref(), &session.literal)?;
            Some((all[t].spot.clone(), all[t].rect, slot(all, t, &session.literal, fx, fy)))
        });
        match (session.over.clone(), found) {
            (Some(o), Some((spot, rect, slot))) if o.spot.path == spot.path => {
                if o.slot != slot {
                    self.set_over(Some(Over { spot, rect, slot }));
                    self.fire_over(DragEvent::Over, (fx, fy));
                }
            }
            (old, new) => {
                if let Some(o) = old {
                    let session = self.drag_drop.session.clone().expect("a drag is under way");
                    self.fire_dnd(
                        &o.spot,
                        o.rect,
                        DragEvent::Leave,
                        (fx, fy),
                        Fields { session: &session, slot: Some(&o.slot), dropped: false },
                    );
                }
                let entered = new.is_some();
                self.set_over(new.map(|(spot, rect, slot)| Over { spot, rect, slot }));
                if entered {
                    self.fire_over(DragEvent::Enter, (fx, fy));
                    self.fire_over(DragEvent::Over, (fx, fy));
                }
            }
        }
    }

    fn set_over(&mut self, over: Option<Over>) {
        if let Some(s) = self.drag_drop.session.as_mut() {
            s.over = over;
        }
        self.dnd_interaction();
    }

    /// An event on the target the drag is over now.
    fn fire_over(&mut self, event: DragEvent, at: (f32, f32)) {
        let Some(session) = self.drag_drop.session.clone() else { return };
        let Some(o) = &session.over else { return };
        self.fire_dnd(&o.spot, o.rect, event, at, Fields { session: &session, slot: Some(&o.slot), dropped: false });
    }

    /// `:dragging` and `:drag-over` follow the drag.
    fn dnd_interaction(&mut self) {
        let mut next = self.document.interaction().clone();
        let session = self.drag_drop.session.as_ref();
        next.dragging = session.map(|s| s.literal.clone());
        next.drag_over = session.and_then(|s| s.over.as_ref().map(|o| o.spot.path.clone()));
        if self.document.set_interaction(next) {
            self.request_redraw();
        }
    }

    /// The button or finger came up. Whether a drag has finished, so the press
    /// is not also a release of anything else.
    pub(crate) fn dnd_release(&mut self, fx: f32, fy: f32) -> bool {
        self.drag_drop.pending = None;
        if self.drag_drop.session.is_none() {
            return false;
        }
        self.dnd_track(fx, fy);
        let Some(session) = self.drag_drop.session.clone() else { return true };
        let dropped = session.over.is_some();
        if let Some(o) = &session.over {
            self.fire_dnd(
                &o.spot,
                o.rect,
                DragEvent::Drop,
                (fx, fy),
                Fields { session: &session, slot: Some(&o.slot), dropped: true },
            );
        }
        self.finish_drag((fx, fy), dropped);
        true
    }

    /// Escape, or the system took the finger: the drag is over and nothing was
    /// dropped. Whether there was one.
    pub(crate) fn dnd_cancel(&mut self) -> bool {
        self.drag_drop.pending = None;
        let Some(session) = self.drag_drop.session.clone() else { return false };
        let at = (session.rect.0 + session.grab.0, session.rect.1 + session.grab.1);
        if let Some(o) = &session.over {
            self.fire_dnd(
                &o.spot,
                o.rect,
                DragEvent::Leave,
                at,
                Fields { session: &session, slot: Some(&o.slot), dropped: false },
            );
        }
        self.finish_drag(at, false);
        true
    }

    /// `@dragend` on the source, found again by what it carries (a live move
    /// has rebuilt the list it was in), and the session is gone.
    fn finish_drag(&mut self, at: (f32, f32), dropped: bool) {
        let Some(session) = self.drag_drop.session.clone() else { return };
        let now = self.with_placed(|all| {
            all.iter()
                .find(|p| p.spot.dnd.data.as_deref() == Some(&session.literal))
                .map(|p| (p.spot.clone(), p.rect))
        });
        let (spot, rect) = now.unwrap_or((session.source.clone(), session.rect));
        let slot = session.over.as_ref().map(|o| &o.slot);
        self.fire_dnd(&spot, rect, DragEvent::End, at, Fields { session: &session, slot, dropped });
        self.drag_drop.session = None;
        self.dnd_interaction();
        self.gesture = None;
        self.gesture_deadline = None;
    }

    /// Run one drag event's handler on `spot`, if it wrote one.
    fn fire_dnd(&mut self, spot: &Spot, rect: Rect, event: DragEvent, at: (f32, f32), f: Fields) {
        let Some(body) = spot.dnd.handler(event) else { return };
        let mut ev = self.pointer_event(at.0, at.1, (rect.0, rect.1), (rect.2, rect.3), 0);
        let before = f
            .slot
            .and_then(|s| s.before.as_deref())
            .and_then(|lit| self.document.eval_literal(lit))
            .unwrap_or(Value::Null);
        if let Value::Map(fields) = &mut ev {
            fields.extend([
                ("data".to_string(), f.session.value.clone()),
                ("group".to_string(), f.session.group.clone().map_or(Value::Null, Value::Text)),
                ("index".to_string(), Value::Number(f.slot.map_or(-1.0, |s| s.index as f64))),
                ("before".to_string(), before),
                ("dropped".to_string(), Value::Bool(f.dropped)),
                ("grabX".to_string(), Value::Number(f.session.grab.0 as f64)),
                ("grabY".to_string(), Value::Number(f.session.grab.1 as f64)),
            ]);
        }
        if self.document.apply_handler_with_event(body, spot.instance.as_deref(), &ev) {
            self.request_redraw();
        }
        self.adopt_element_requests();
    }
}
