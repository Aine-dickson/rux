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
#[derive(Clone, Copy, Debug)]
enum AnimValue {
    Num(f32),
    Color(Rgba),
    /// Four sides or four corners.
    Quad([f32; 4]),
    Len(Len),
    Insets([Option<Len>; 4]),
    /// An affine `transform`.
    Mat([f32; 6]),
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
            let target = if close(&authored, &track.written) { track.target } else { authored };
            if !close(&target, &track.target) {
                let current = match driven {
                    Some(t) => track.value_at_progress(t),
                    None => track.value_at(now),
                };
                *track = Track::starting(current, target, *spec, now);
            }

            if let Some(t) = driven {
                let value = track.value_at_progress(t);
                write(node, *prop, value);
                track.written = value;
                continue;
            }

            if track.active {
                let value = track.value_at(now);
                if now >= track.end_ms {
                    track.active = false;
                }
                write(node, *prop, value);
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
            target: value,
            written: value,
            from: value,
            start_ms: 0.0,
            end_ms: 0.0,
            easing: Easing::Linear,
            active: false,
        }
    }

    fn starting(from: AnimValue, target: AnimValue, spec: Transition, now: f64) -> Self {
        let start = now + spec.delay as f64;
        let mut track = Self {
            target,
            written: from,
            from,
            start_ms: start,
            end_ms: start + spec.duration.max(0.0) as f64,
            easing: spec.easing,
            active: spec.duration > 0.0,
        };
        // `transition: opacity 0s` is how CSS turns a transition off without
        // deleting the declaration, and a zero-length interpolation would be a
        // division by zero here.
        if !track.active {
            track.written = target;
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
    fn value_at_progress(&self, t: f32) -> AnimValue {
        let eased = self.easing.eval(t.clamp(0.0, 1.0));
        lerp(&self.from, &self.target, eased).unwrap_or(self.target)
    }

    fn value_at(&self, now: f64) -> AnimValue {
        if !self.active || now <= self.start_ms {
            return self.from;
        }
        if now >= self.end_ms {
            return self.target;
        }
        let t = (now - self.start_ms) / (self.end_ms - self.start_ms);
        let eased = self.easing.eval(t as f32);
        lerp(&self.from, &self.target, eased).unwrap_or(self.target)
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
