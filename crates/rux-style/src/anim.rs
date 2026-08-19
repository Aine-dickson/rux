//! Transitions: the part of animation that happens *between two builds*.
//!
//! A build produces a whole fresh tree with every style computed from scratch,
//! so nothing in the tree remembers what a property used to be. The [`Animator`]
//! is that memory. It sits between the build and the layout: each frame it looks
//! at what the build asked for, compares it against what the node was showing,
//! and writes back an interpolated value while a transition is in flight.
//!
//! Two properties of the design are worth keeping.
//!
//! **The animator is the only thing that knows about time here.** It takes the
//! clock as an argument (`now_ms`) rather than reading one, so it is testable
//! without a window and works unchanged on the web, where `Instant` is not the
//! clock the event loop uses.
//!
//! **It owns its state, and the shell owns it.** Per-frame state does not belong
//! in the document; the document is rebuilt out from under it constantly. The
//! tree is where the animator *writes*, never where it remembers.

use std::collections::HashMap;

use rux_layout::PathCmd;
use rux_layout::{AnimProp, Background, Easing, Len, Node, Rgba, Sides, Transition};

/// How soon to ask for the next frame while something is in flight. The shell
/// presents with vsync, so this is a floor rather than a frame rate: it exists
/// so a missing vsync cannot turn into a busy loop.
pub const FRAME_MS: f64 = 4.0;

/// One segment of a node's identity.
///
/// Position, except across a keyed list, where the `r-key` is the identity and
/// the position is not: reordering rows must carry each row's in-flight
/// transition with it rather than leaving it behind with the slot. This is the
/// same reasoning the caret already follows.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Seg {
    Index(usize),
    Key(String),
}

/// A value being interpolated. One variant per shape of thing, not per property.
///
/// Not `Copy`, because path geometry is a list. Everything else here still is,
/// and the clones this costs are one per animating property per frame, on the
/// nodes that declared a transition and no others.
#[derive(Clone, Debug)]
enum AnimValue {
    Num(f32),
    Color(Rgba),
    /// Four sides or four corners.
    Quad([f32; 4]),
    Len(Len),
    Insets([Option<Len>; 4]),
    /// An affine `transform`.
    Mat([f32; 6]),
    /// `<path>` geometry. Interpolates against another path with the same
    /// command sequence and jumps otherwise, which is the rule every other
    /// value here already follows.
    Path(Vec<PathCmd>),
}

/// What one property of one node is doing.
#[derive(Clone, Debug)]
struct Track {
    /// The value the build last asked for.
    target: AnimValue,
    /// The value this animator last wrote into the tree.
    ///
    /// The guard against reading our own output back as an authored change: on
    /// the next frame the tree holds *this*, not the target, and without
    /// remembering it the animator would see a difference every frame and
    /// restart the transition forever, never arriving.
    written: AnimValue,
    from: AnimValue,
    /// When interpolation begins (the delay has already been added) and ends.
    start_ms: f64,
    end_ms: f64,
    easing: Easing,
    active: bool,
    /// Whether the last frame positioned this track from someone else's
    /// progress rather than from the clock.
    ///
    /// Handing a swap back to the clock has to *restart* the interpolation from
    /// where the drag left it. Without this the track still holds the deadline
    /// it was given when the swap opened, that deadline is long past, and the
    /// element jumps to the end instead of settling into it.
    driven: bool,
}

#[derive(Default)]
struct NodeState {
    touched: bool,
    props: HashMap<AnimProp, Track>,
}

/// Per-node memory of what each transitioned property was showing.
#[derive(Default)]
pub struct Animator {
    nodes: HashMap<Vec<Seg>, NodeState>,
}

impl Animator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether anything is currently animating. Used to decide whether a frame
    /// needs to be scheduled at all.
    pub fn is_idle(&self) -> bool {
        self.nodes.values().all(|n| n.props.values().all(|t| !t.active))
    }

    /// Fold this frame into `root`: start transitions for any transitioned
    /// property whose target moved since the last frame, and overwrite the
    /// styles of everything in flight with their value at `now_ms`.
    ///
    /// Returns how many milliseconds until the next frame is needed, or `None`
    /// when nothing is animating and the app can go back to sleep. That `None`
    /// is the point: an idle Rux app must not burn frames.
    pub fn apply(&mut self, root: &mut Node, now_ms: f64) -> Option<f64> {
        for state in self.nodes.values_mut() {
            state.touched = false;
        }
        let mut wake: Option<f64> = None;
        let mut path = Vec::new();
        self.visit(root, &mut path, now_ms, &mut wake);
        // A node the build no longer reaches is gone, and so is its memory. It
        // gets no leaving animation: that is tier 2, and pretending otherwise
        // here would animate a node nothing is drawing.
        self.nodes.retain(|_, state| state.touched);
        wake
    }

    fn visit(&mut self, node: &mut Node, path: &mut Vec<Seg>, now: f64, wake: &mut Option<f64>) {
        if !node.style.transitions.is_empty() {
            self.node_frame(node, path, now, wake);
        }
        for (i, child) in node.children.iter_mut().enumerate() {
            path.push(match &child.key {
                Some(k) => Seg::Key(k.clone()),
                None => Seg::Index(i),
            });
            self.visit(child, path, now, wake);
            path.pop();
        }
    }

    /// One transitioning node, one frame.
    fn node_frame(&mut self, node: &mut Node, path: &[Seg], now: f64, wake: &mut Option<f64>) {
        // A swap someone else is driving: position is a function of their value
        // and not of the clock at all. Nothing here waits for it, and nothing
        // here ends it either, because a driven swap can travel back the way it
        // came and finishing it is the runtime's call rather than the
        // animator's.
        let driven = node.style.swap_progress;
        let specs = specs_of(&node.style.transitions);
        let state = self.nodes.entry(path.to_vec()).or_default();
        state.touched = true;
        // A property that stopped being transitioned stops being remembered, so
        // re-adding the declaration later starts from what is on screen.
        state.props.retain(|prop, _| specs.contains_key(prop));

        for (prop, spec) in &specs {
            let Some(authored) = read(node, *prop) else { continue };
            let Some(track) = state.props.get_mut(prop) else {
                // First sight of this node: it arrives at its authored value.
                // Animating here would animate every element into existence,
                // which is enter/leave, and deliberately not this tier.
                state.props.insert(*prop, Track::settled(authored));
                continue;
            };

            // The tree holds either our own last write (nothing was rebuilt, or
            // it was rebuilt to the same target) or a new authored value.
            let target = if close(&authored, &track.written) { track.target.clone() } else { authored };
            // Where the element is *now*, which is where any restart has to
            // begin from.
            //
            // For a track that has been driven this must be what was last
            // written, and the distinction is the whole of a released swipe. Its
            // deadline was set when the swap opened and is long past by the time
            // a finger lets go, so `value_at(now)` would answer "finished" and
            // the element would jump to the far end before travelling back. The
            // clock is meaningless for something a hand has been holding.
            let current = if track.driven {
                track.written.clone()
            } else if let Some(t) = driven {
                track.value_at_progress(t)
            } else {
                track.value_at(now)
            };
            if !close(&target, &track.target) {
                *track = Track::starting(current, target.clone(), *spec, now);
            } else if driven.is_none() && track.driven {
                // Handed back to the clock with the target unmoved: pick the
                // interpolation up from where the drag left it and run the rest
                // on time.
                *track = Track::starting(current, track.target.clone(), *spec, now);
            }

            if let Some(t) = driven {
                let value = track.value_at_progress(t);
                write(node, *prop, value.clone());
                track.written = value;
                track.driven = true;
                continue;
            }
            track.driven = false;

            if track.active {
                let value = track.value_at(now);
                if now >= track.end_ms {
                    track.active = false;
                }
                write(node, *prop, value.clone());
                track.written = value;
                // The frame that lands on the target is the last one: it is
                // painted, and then there is nothing left to wake for. Before
                // the delay elapses there is likewise nothing to draw, so sleep
                // until it is due rather than spinning through frames that all
                // paint the same pixel.
                if track.active {
                    let next = if now < track.start_ms { track.start_ms - now } else { FRAME_MS };
                    *wake = Some(wake.map_or(next, |w: f64| w.min(next)));
                }
            } else {
                track.written = target;
            }
        }
    }
}

impl Track {
    /// A property that is not animating: it is showing what was asked for.
    fn settled(value: AnimValue) -> Self {
        Self {
            target: value.clone(),
            written: value.clone(),
            from: value,
            start_ms: 0.0,
            end_ms: 0.0,
            easing: Easing::Linear,
            active: false,
            driven: false,
        }
    }

    fn starting(from: AnimValue, target: AnimValue, spec: Transition, now: f64) -> Self {
        let start = now + spec.delay as f64;
        let mut track = Self {
            target: target.clone(),
            written: from.clone(),
            from: from.clone(),
            start_ms: start,
            end_ms: start + spec.duration.max(0.0) as f64,
            easing: spec.easing,
            active: spec.duration > 0.0,
            driven: false,
        };
        // `transition: opacity 0s` is how CSS turns a transition off without
        // deleting the declaration, and a zero-length interpolation would be a
        // division by zero here.
        if !track.active {
            track.written = target.clone();
        }
        // Two values that cannot be interpolated (a colour becoming a gradient,
        // `10px` becoming `50%`) jump. Better an honest jump than a frame of
        // nonsense: `lerp` returns `None` and says so.
        if lerp(&from, &target, 0.5).is_none() {
            track.active = false;
            track.written = target;
        }
        track
    }

    /// Where this track sits at a progress somebody else chose, 0 being the
    /// value the swap started from and 1 the value it is heading for.
    ///
    /// The declared duration is ignored here, and that is the division of
    /// labour: `transition` says *which* properties take part in a swap, and
    /// the driver says how far along it is.
    ///
    /// **The easing is ignored too, and that is not an oversight.** An easing is
    /// a *timing* function: it maps elapsed time onto progress. A driver that
    /// supplies progress directly has already done that job, and running the
    /// curve over it again means the element does not track the hand holding it.
    /// With `ease-out`, a third of the way through a drag put the element nearly
    /// two thirds of the way gone, which reads as the thing being destroyed
    /// before the gesture is finished.
    fn value_at_progress(&self, t: f32) -> AnimValue {
        lerp(&self.from, &self.target, t.clamp(0.0, 1.0)).unwrap_or_else(|| self.target.clone())
    }

    fn value_at(&self, now: f64) -> AnimValue {
        if !self.active || now <= self.start_ms {
            return self.from.clone();
        }
        if now >= self.end_ms {
            return self.target.clone();
        }
        let t = (now - self.start_ms) / (self.end_ms - self.start_ms);
        let eased = self.easing.eval(t as f32);
        lerp(&self.from, &self.target, eased).unwrap_or_else(|| self.target.clone())
    }
}

/// The per-property declarations in force, with `all` expanded and later entries
/// winning, as the cascade does inside a single declaration.
fn specs_of(transitions: &[Transition]) -> HashMap<AnimProp, Transition> {
    let mut out = HashMap::new();
    for t in transitions {
        if t.property == AnimProp::All {
            for prop in AnimProp::EVERY {
                out.insert(*prop, Transition { property: *prop, ..*t });
            }
        } else {
            out.insert(t.property, *t);
        }
    }
    out
}

/// This node's current value for `prop`, or `None` when the property is not
/// something this node has (text colour on a box with no text) or not in an
/// interpolable form (a gradient background).
fn read(node: &Node, prop: AnimProp) -> Option<AnimValue> {
    let st = &node.style;
    Some(match prop {
        AnimProp::All => return None, // expanded away by `specs_of`
        AnimProp::Opacity => AnimValue::Num(st.opacity),
        AnimProp::BackgroundColor => match st.background {
            Some(Background::Color(c)) => AnimValue::Color(c),
            _ => return None,
        },
        AnimProp::Color => AnimValue::Color(node.text.as_ref()?.color),
        AnimProp::BorderColor => AnimValue::Color(st.border_color?),
        AnimProp::BorderWidth => AnimValue::Quad(quad(st.border)),
        AnimProp::BorderRadius => AnimValue::Quad(st.radius),
        AnimProp::Width => AnimValue::Len(st.width?),
        AnimProp::Height => AnimValue::Len(st.height?),
        AnimProp::Padding => AnimValue::Quad(quad(st.padding)),
        AnimProp::Margin => AnimValue::Quad(quad(st.margin)),
        AnimProp::Gap => AnimValue::Num(st.gap),
        AnimProp::FontSize => AnimValue::Num(node.text.as_ref()?.font_size),
        // An absent `transform` is the identity, which *is* interpolable: that
        // is what makes `transform: scale(1.05)` on hover animate from nothing.
        AnimProp::Transform => AnimValue::Mat(st.transform.unwrap_or(IDENTITY)),
        AnimProp::Inset => AnimValue::Insets(st.inset),
        // `fill: none` is an absence rather than a transparent colour, so there
        // is nothing to walk between and the change lands at once. Fading a
        // fill out is `transparent`, which is a colour and does interpolate.
        AnimProp::Fill => AnimValue::Color(st.fill?),
        AnimProp::Stroke => AnimValue::Color(st.stroke?),
        AnimProp::StrokeWidth => AnimValue::Num(st.stroke_width),
        AnimProp::PathData => AnimValue::Path(node.path.as_ref()?.commands.clone()),
    })
}

fn write(node: &mut Node, prop: AnimProp, value: AnimValue) {
    let st = &mut node.style;
    match (prop, value) {
        (AnimProp::Opacity, AnimValue::Num(v)) => st.opacity = v.clamp(0.0, 1.0),
        (AnimProp::BackgroundColor, AnimValue::Color(c)) => st.background = Some(Background::Color(c)),
        (AnimProp::Color, AnimValue::Color(c)) => {
            if let Some(text) = node.text.as_mut() {
                text.color = c;
            }
        }
        (AnimProp::BorderColor, AnimValue::Color(c)) => st.border_color = Some(c),
        (AnimProp::BorderWidth, AnimValue::Quad(q)) => st.border = sides(q),
        (AnimProp::BorderRadius, AnimValue::Quad(q)) => st.radius = q,
        (AnimProp::Width, AnimValue::Len(l)) => st.width = Some(l),
        (AnimProp::Height, AnimValue::Len(l)) => st.height = Some(l),
        (AnimProp::Padding, AnimValue::Quad(q)) => st.padding = sides(q),
        (AnimProp::Margin, AnimValue::Quad(q)) => st.margin = sides(q),
        (AnimProp::Gap, AnimValue::Num(v)) => st.gap = v,
        (AnimProp::FontSize, AnimValue::Num(v)) => {
            if let Some(text) = node.text.as_mut() {
                text.font_size = v;
            }
        }
        (AnimProp::Transform, AnimValue::Mat(m)) => st.transform = Some(m),
        (AnimProp::Inset, AnimValue::Insets(i)) => st.inset = i,
        (AnimProp::Fill, AnimValue::Color(c)) => st.fill = Some(c),
        (AnimProp::Stroke, AnimValue::Color(c)) => st.stroke = Some(c),
        (AnimProp::StrokeWidth, AnimValue::Num(v)) => st.stroke_width = v.max(0.0),
        (AnimProp::PathData, AnimValue::Path(cmds)) => {
            if let Some(path) = node.path.as_mut() {
                // The commands only. `d` stays as the author wrote it, because
                // it is the identity a later build compares against, and
                // rewriting it to whatever this frame interpolated to would
                // make every frame look like a fresh authored change.
                path.commands = cmds;
            }
        }
        // A value of the wrong shape for its property cannot be produced:
        // `read` and `lerp` both preserve the variant.
        _ => {}
    }
}

const IDENTITY: [f32; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

fn quad(s: Sides) -> [f32; 4] {
    [s.top, s.right, s.bottom, s.left]
}

fn sides(q: [f32; 4]) -> Sides {
    Sides { top: q[0], right: q[1], bottom: q[2], left: q[3] }
}

/// Interpolate `a` → `b` at eased progress `t`, or `None` when the two cannot be
/// interpolated at all, which is what makes the transition snap instead.
///
/// A transform is interpolated coefficient by coefficient. That is exact for
/// translation and scale, which is nearly everything anyone writes; a rotation
/// through a large angle passes through a slightly squashed intermediate rather
/// than sweeping the arc, since the decomposed form CSS uses is a much larger
/// piece of machinery than tier 1 is buying.
fn lerp(a: &AnimValue, b: &AnimValue, t: f32) -> Option<AnimValue> {
    let f = |x: f32, y: f32| x + (y - x) * t;
    Some(match (a, b) {
        (AnimValue::Num(x), AnimValue::Num(y)) => AnimValue::Num(f(*x, *y)),
        (AnimValue::Color(x), AnimValue::Color(y)) => AnimValue::Color(Rgba {
            r: f(x.r, y.r),
            g: f(x.g, y.g),
            b: f(x.b, y.b),
            a: f(x.a, y.a),
        }),
        (AnimValue::Quad(x), AnimValue::Quad(y)) => {
            AnimValue::Quad([f(x[0], y[0]), f(x[1], y[1]), f(x[2], y[2]), f(x[3], y[3])])
        }
        (AnimValue::Mat(x), AnimValue::Mat(y)) => {
            let mut m = [0.0; 6];
            for i in 0..6 {
                m[i] = f(x[i], y[i]);
            }
            AnimValue::Mat(m)
        }
        (AnimValue::Len(x), AnimValue::Len(y)) => AnimValue::Len(lerp_len(*x, *y, t)?),
        (AnimValue::Path(x), AnimValue::Path(y)) => AnimValue::Path(rux_layout::path::lerp(x, y, t)?),
        (AnimValue::Insets(x), AnimValue::Insets(y)) => {
            let mut out = [None; 4];
            for i in 0..4 {
                out[i] = match (x[i], y[i]) {
                    (Some(p), Some(q)) => Some(lerp_len(p, q, t)?),
                    // `auto` is not a number, so there is nothing to walk
                    // between: the side takes its new value at once.
                    _ => y[i],
                };
            }
            AnimValue::Insets(out)
        }
        _ => return None,
    })
}

/// Two lengths interpolate only in the same unit. `10px` → `50%` needs the
/// containing block to resolve, which is a layout the animator runs *before*.
fn lerp_len(a: Len, b: Len, t: f32) -> Option<Len> {
    let f = |x: f32, y: f32| x + (y - x) * t;
    Some(match (a, b) {
        (Len::Px(x), Len::Px(y)) => Len::Px(f(x, y)),
        (Len::Pct(x), Len::Pct(y)) => Len::Pct(f(x, y)),
        (Len::Vw(x), Len::Vw(y)) => Len::Vw(f(x, y)),
        (Len::Vh(x), Len::Vh(y)) => Len::Vh(f(x, y)),
        _ => return None,
    })
}

/// Whether two values are the same to within what a screen can show.
///
/// An exact comparison would be wrong in both directions: the animator's own
/// interpolation lands a hair off its target, and a rebuild recomputes an
/// unchanged value through the same float arithmetic and can land a hair off
/// that.
fn close(a: &AnimValue, b: &AnimValue) -> bool {
    const EPS: f32 = 1e-4;
    let near = |x: f32, y: f32| (x - y).abs() < EPS;
    match (a, b) {
        (AnimValue::Num(x), AnimValue::Num(y)) => near(*x, *y),
        (AnimValue::Color(x), AnimValue::Color(y)) => {
            near(x.r, y.r) && near(x.g, y.g) && near(x.b, y.b) && near(x.a, y.a)
        }
        (AnimValue::Quad(x), AnimValue::Quad(y)) => x.iter().zip(y).all(|(p, q)| near(*p, *q)),
        (AnimValue::Mat(x), AnimValue::Mat(y)) => x.iter().zip(y).all(|(p, q)| near(*p, *q)),
        (AnimValue::Len(x), AnimValue::Len(y)) => close_len(*x, *y),
        (AnimValue::Insets(x), AnimValue::Insets(y)) => x.iter().zip(y).all(|(p, q)| match (p, q) {
            (Some(p), Some(q)) => close_len(*p, *q),
            (None, None) => true,
            _ => false,
        }),
        // Command by command, and the shapes must agree on the commands
        // themselves: a curve is never close to a move.
        //
        // **A missing arm here does not degrade, it freezes.** This is the
        // guard that tells our own last write apart from a fresh authored
        // value, so falling through to `false` means every frame decides the
        // author has just changed the geometry, restarts the track, and sets
        // the target to whatever this frame had already interpolated to. The
        // shape sets off, converges on itself and stops. Found by watching
        // `examples/morph.rux` do exactly that, with the whole suite green.
        (AnimValue::Path(x), AnimValue::Path(y)) => {
            x.len() == y.len()
                && x.iter().zip(y).all(|(p, q)| match (p, q) {
                    (PathCmd::Move { x: ax, y: ay }, PathCmd::Move { x: bx, y: by }) => {
                        near(*ax, *bx) && near(*ay, *by)
                    }
                    (
                        PathCmd::Curve { x1: a1, y1: b1, x2: a2, y2: b2, x: ax, y: ay },
                        PathCmd::Curve { x1: c1, y1: d1, x2: c2, y2: d2, x: bx, y: by },
                    ) => {
                        near(*a1, *c1)
                            && near(*b1, *d1)
                            && near(*a2, *c2)
                            && near(*b2, *d2)
                            && near(*ax, *bx)
                            && near(*ay, *by)
                    }
                    (PathCmd::Close, PathCmd::Close) => true,
                    _ => false,
                })
        }
        _ => false,
    }
}

fn close_len(a: Len, b: Len) -> bool {
    const EPS: f32 = 1e-4;
    match (a, b) {
        (Len::Px(x), Len::Px(y))
        | (Len::Pct(x), Len::Pct(y))
        | (Len::Vw(x), Len::Vw(y))
        | (Len::Vh(x), Len::Vh(y)) => (x - y).abs() < EPS,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rux_layout::Style;

    /// A one-node tree with a transition on `opacity`.
    fn fading(opacity: f32, duration: f32) -> Node {
        let mut style = Style { opacity, ..Style::default() };
        style.transitions = vec![Transition {
            property: AnimProp::Opacity,
            duration,
            delay: 0.0,
            easing: Easing::Linear,
        }];
        Node::new(style)
    }

    /// A shape keeps walking across frames where nothing was rebuilt.
    ///
    /// The regression test for a freeze found by *watching* `examples/morph.rux`
    /// while the whole suite was green. Between builds the tree holds the
    /// animator's own last write, so `read` hands back an interpolated shape
    /// rather than the authored one. `close` is what tells those apart, and
    /// with no `Path` arm it answered "not close" every frame: the animator
    /// decided the author had changed the geometry, restarted the track, and
    /// set the target to the value it had itself just written. The square set
    /// off towards the circle, converged on wherever it had reached, and
    /// stopped there for good.
    ///
    /// The shape of the bug is the point: a missing comparison arm does not
    /// make an animation slightly wrong, it stops it dead partway.
    #[test]
    fn a_morphing_shape_does_not_stall_on_its_own_output() {
        use rux_layout::PathContent;

        let mut style = Style::default();
        style.transitions = vec![Transition {
            property: AnimProp::PathData,
            duration: 400.0,
            delay: 0.0,
            easing: Easing::Linear,
        }];
        let square = PathContent::parse("M 50 0 L 100 0 L 100 100 L 0 100 L 0 0 Z");
        let circle = PathContent::parse(
            "M 50 0 A 50 50 0 0 1 100 50 A 50 50 0 0 1 50 100 A 50 50 0 0 1 0 50 A 50 50 0 0 1 50 0 Z",
        );
        let mut node = Node::path(style, square.clone());

        let mut anim = Animator::new();
        assert_eq!(anim.apply(&mut node, 0.0), None, "settled as built");

        // The build asks for the circle. From here on nothing rebuilds, which
        // is the whole point: the tree carries the animator's own output.
        node.path = Some(circle.clone());
        anim.apply(&mut node, 0.0);

        let at = |n: &Node| n.path.as_ref().unwrap().commands.clone();
        let quarter = {
            anim.apply(&mut node, 100.0);
            at(&node)
        };
        assert_ne!(quarter, square.commands, "it set off");
        assert_ne!(quarter, circle.commands, "and is not there yet");

        // The frame that used to freeze. `read` sees `quarter`, which the
        // animator wrote, not the circle the author asked for.
        anim.apply(&mut node, 200.0);
        let half = at(&node);
        assert_ne!(half, quarter, "it kept going rather than stalling");

        // And it arrives, on time.
        anim.apply(&mut node, 400.0);
        assert_eq!(at(&node), circle.commands, "the walk finished at the circle");
    }

    /// A node carrying `swap_progress` is positioned by that value and not by
    /// the clock: it does not move when time passes, and it does move when the
    /// value does. This is what lets a finger drive an enter/leave swap.
    #[test]
    fn a_driven_swap_follows_its_progress_and_not_the_clock() {
        let mut anim = Animator::new();
        let mut node = fading(1.0, 300.0);
        assert_eq!(anim.apply(&mut node, 0.0), None, "settled where it was built");

        // The swap opens: the build now asks for 0, and hands over progress.
        node.style.opacity = 0.0;
        node.style.swap_progress = Some(0.25);
        assert_eq!(anim.apply(&mut node, 0.0), None, "driven, so nothing to wake for");
        assert!(
            (node.style.opacity - 0.75).abs() < 1e-4,
            "a quarter of the way from 1 to 0: {}",
            node.style.opacity
        );

        // Time alone moves it nowhere.
        node.style.opacity = 0.0;
        node.style.swap_progress = Some(0.25);
        let _ = anim.apply(&mut node, 10_000.0);
        assert!(
            (node.style.opacity - 0.75).abs() < 1e-4,
            "still a quarter of the way, ten seconds later: {}",
            node.style.opacity
        );

        // And it can travel back the way it came, which a clock cannot do.
        node.style.opacity = 0.0;
        node.style.swap_progress = Some(0.1);
        let _ = anim.apply(&mut node, 10_100.0);
        assert!(
            (node.style.opacity - 0.9).abs() < 1e-4,
            "reversed: {}",
            node.style.opacity
        );
    }

    /// Handing a driven swap back to the clock settles from where the drag left
    /// it, rather than jumping to the end.
    ///
    /// The jump is what happens if the track keeps the deadline it was given
    /// when the swap opened: by the time a finger lets go that deadline is long
    /// past, so every remaining frame reads as "already finished".
    #[test]
    fn a_swap_handed_back_to_the_clock_settles_from_where_it_is() {
        let mut anim = Animator::new();
        let mut node = fading(1.0, 100.0);
        assert_eq!(anim.apply(&mut node, 0.0), None);

        // Dragged 40% of the way out and held there for a while.
        node.style.opacity = 0.0;
        node.style.swap_progress = Some(0.4);
        let _ = anim.apply(&mut node, 500.0);
        assert!((node.style.opacity - 0.6).abs() < 1e-4, "{}", node.style.opacity);

        // Let go: no progress any more, so the clock takes over from 0.6.
        node.style.opacity = 0.0;
        node.style.swap_progress = None;
        let wake = anim.apply(&mut node, 500.0);
        assert!(wake.is_some(), "it is animating again, so a frame is due");
        assert!(
            (node.style.opacity - 0.6).abs() < 1e-4,
            "still where the finger left it on the handover frame: {}",
            node.style.opacity
        );

        // Half of the declared 100ms later, half of the *remaining* distance.
        node.style.opacity = 0.0;
        let _ = anim.apply(&mut node, 550.0);
        assert!(
            node.style.opacity > 0.25 && node.style.opacity < 0.35,
            "settling, not jumped: {}",
            node.style.opacity
        );

        node.style.opacity = 0.0;
        let _ = anim.apply(&mut node, 600.0);
        assert!((node.style.opacity - 0.0).abs() < 1e-4, "arrived: {}", node.style.opacity);
    }

    /// A reversed drag starts back from **where the finger left it**, not from
    /// the far end.
    ///
    /// The bug this pins was reported from the window and is invisible to the
    /// other tests: on reversal the target moves (`:leave-to` back to the base
    /// style) on the *same frame* that the driver lets go. The target-moved
    /// branch then restarted the interpolation from `value_at(now)`, and `now`
    /// was long past a deadline set when the swap opened, so it answered "you
    /// have finished" and the element jumped to fully gone before travelling
    /// home. It read as the thing being destroyed and a fresh one flying back.
    #[test]
    fn a_reversed_drag_starts_from_where_the_finger_left_it() {
        let mut anim = Animator::new();
        let mut node = fading(1.0, 300.0);
        assert_eq!(anim.apply(&mut node, 0.0), None);

        // Dragged a third of the way out, over a while, so the swap's own
        // deadline is comfortably in the past.
        node.style.opacity = 0.0;
        node.style.swap_progress = Some(0.33);
        let _ = anim.apply(&mut node, 2_000.0);
        assert!((node.style.opacity - 0.67).abs() < 1e-3, "{}", node.style.opacity);

        // Released short of the threshold: the author puts the condition back,
        // so the target returns to the base style, and hands progress to the
        // clock. Both land on the same frame.
        node.style.opacity = 1.0;
        node.style.swap_progress = None;
        let _ = anim.apply(&mut node, 2_000.0);
        assert!(
            (node.style.opacity - 0.67).abs() < 1e-3,
            "it sets off from where it was, not from gone: {}",
            node.style.opacity
        );

        // And walks home from there rather than appearing at the other end.
        node.style.opacity = 1.0;
        let _ = anim.apply(&mut node, 2_150.0);
        let half = node.style.opacity;
        assert!(half > 0.67 && half < 1.0, "on its way back: {half}");
    }

    /// Driven progress is **not** eased. An easing maps time onto progress, and
    /// a driver has already supplied progress, so easing it again means the
    /// element does not track the hand holding it.
    #[test]
    fn a_driven_swap_tracks_its_progress_one_to_one() {
        let mut anim = Animator::new();
        let mut style = Style { opacity: 1.0, ..Style::default() };
        style.transitions = vec![Transition {
            property: AnimProp::Opacity,
            duration: 300.0,
            delay: 0.0,
            // Deliberately a curve with a lot of shape to it.
            easing: Easing::EASE_OUT,
        }];
        let mut node = Node::new(style);
        assert_eq!(anim.apply(&mut node, 0.0), None);

        node.style.opacity = 0.0;
        node.style.swap_progress = Some(0.25);
        let _ = anim.apply(&mut node, 0.0);
        assert!(
            (node.style.opacity - 0.75).abs() < 1e-4,
            "a quarter dragged is a quarter moved, not {} (eased would be ~0.45)",
            node.style.opacity
        );
    }

    #[test]
    fn a_style_change_between_builds_is_walked_rather_than_jumped() {
        let mut anim = Animator::new();
        let mut node = fading(1.0, 100.0);

        // First frame: the node arrives at what it was built as. Nothing
        // animates on first sight.
        assert_eq!(anim.apply(&mut node, 0.0), None);
        assert_eq!(node.style.opacity, 1.0);

        // A build sets the target to 0. The frame it happens on still shows the
        // old value: that is the start of the walk, not the end of it.
        node.style.opacity = 0.0;
        let wake = anim.apply(&mut node, 0.0);
        assert_eq!(wake, Some(FRAME_MS));
        assert_eq!(node.style.opacity, 1.0);

        // Halfway, on a linear curve.
        anim.apply(&mut node, 50.0);
        assert!((node.style.opacity - 0.5).abs() < 1e-3, "{}", node.style.opacity);

        // The end lands exactly on the target and stops asking for frames.
        assert_eq!(anim.apply(&mut node, 100.0), None);
        assert_eq!(node.style.opacity, 0.0);
        assert!(anim.is_idle());
    }

    #[test]
    fn an_in_flight_value_is_not_read_back_as_a_new_target() {
        // The bug this guards: the animator writes 0.5 into the tree, then next
        // frame sees 0.5 where it expects the target and restarts from there,
        // halving the remaining distance forever and never arriving.
        let mut anim = Animator::new();
        let mut node = fading(1.0, 100.0);
        anim.apply(&mut node, 0.0);
        node.style.opacity = 0.0;
        anim.apply(&mut node, 0.0);

        for step in 1..=10 {
            anim.apply(&mut node, step as f64 * 10.0);
        }
        assert_eq!(node.style.opacity, 0.0);
        assert!(anim.is_idle());
    }

    #[test]
    fn a_target_that_moves_mid_flight_is_followed_from_where_it_is() {
        let mut anim = Animator::new();
        let mut node = fading(1.0, 100.0);
        anim.apply(&mut node, 0.0);
        node.style.opacity = 0.0;
        anim.apply(&mut node, 0.0);
        anim.apply(&mut node, 50.0); // showing 0.5

        // Reversed halfway: it must set off from 0.5, not snap back to 1.
        node.style.opacity = 1.0;
        anim.apply(&mut node, 50.0);
        assert!((node.style.opacity - 0.5).abs() < 1e-3);
        anim.apply(&mut node, 100.0);
        assert!((node.style.opacity - 0.75).abs() < 1e-3, "{}", node.style.opacity);
    }

    #[test]
    fn a_delay_sleeps_until_it_is_due_instead_of_spinning() {
        let mut anim = Animator::new();
        let mut node = fading(1.0, 100.0);
        node.style.transitions[0].delay = 500.0;
        anim.apply(&mut node, 0.0);
        node.style.opacity = 0.0;

        // The wake is the delay itself, not a frame: nothing moves until then.
        assert_eq!(anim.apply(&mut node, 0.0), Some(500.0));
        assert_eq!(node.style.opacity, 1.0);
        assert_eq!(anim.apply(&mut node, 200.0), Some(300.0));
        assert_eq!(node.style.opacity, 1.0);
        anim.apply(&mut node, 550.0);
        assert!((node.style.opacity - 0.5).abs() < 1e-3);
    }

    #[test]
    fn values_that_cannot_be_interpolated_snap() {
        let mut anim = Animator::new();
        let mut style = Style { width: Some(Len::Px(10.0)), ..Style::default() };
        style.transitions = vec![Transition {
            property: AnimProp::Width,
            duration: 100.0,
            delay: 0.0,
            easing: Easing::Linear,
        }];
        let mut node = Node::new(style);
        anim.apply(&mut node, 0.0);

        // px → % has no midpoint without a layout, so it arrives at once and
        // asks for no frames.
        node.style.width = Some(Len::Pct(0.5));
        assert_eq!(anim.apply(&mut node, 0.0), None);
        assert_eq!(node.style.width, Some(Len::Pct(0.5)));
    }

    #[test]
    fn a_keyed_row_keeps_its_transition_when_the_list_reorders() {
        let mut anim = Animator::new();
        let row = |key: &str, opacity: f32| {
            let mut n = fading(opacity, 100.0);
            n.key = Some(key.to_string());
            n
        };
        let mut list = Node::new(Style::default());
        list.children = vec![row("a", 1.0), row("b", 1.0)];
        anim.apply(&mut list, 0.0);

        // `a` starts fading, then the rows swap places.
        list.children[0].style.opacity = 0.0;
        anim.apply(&mut list, 0.0);
        anim.apply(&mut list, 50.0);
        list.children.swap(0, 1);

        // Identity followed the key: the row now in slot 1 is still mid-fade,
        // and the one in slot 0 never started.
        anim.apply(&mut list, 75.0);
        assert!((list.children[1].style.opacity - 0.25).abs() < 1e-3, "{:?}", list.children[1].style.opacity);
        assert_eq!(list.children[0].style.opacity, 1.0);
    }

    #[test]
    fn a_node_the_build_stops_reaching_is_forgotten() {
        let mut anim = Animator::new();
        let mut root = Node::new(Style::default());
        root.children = vec![fading(1.0, 100.0)];
        anim.apply(&mut root, 0.0);
        assert_eq!(anim.nodes.len(), 1);

        root.children.clear();
        anim.apply(&mut root, 0.0);
        assert!(anim.nodes.is_empty());
    }
}
