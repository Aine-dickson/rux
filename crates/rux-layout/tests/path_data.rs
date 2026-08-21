//! The `d` attribute, and what survives parsing it.
//!
//! The whole SVG grammar goes in and three commands come out, so most of what
//! is worth asserting is a conversion: that an arc really is the ellipse it
//! claims to be, that a reflected control really is reflected, and that the
//! spellings which have no separators at all still come apart correctly.

use rux_layout::path::{lerp, parse_path, PathCmd};
use rux_layout::PathContent;

/// Where a command chain ends up, which is what almost every assertion here
/// actually cares about.
fn end(cmds: &[PathCmd]) -> (f32, f32) {
    for c in cmds.iter().rev() {
        match *c {
            PathCmd::Move { x, y } => return (x, y),
            PathCmd::Curve { x, y, .. } => return (x, y),
            PathCmd::Close => {}
        }
    }
    (0.0, 0.0)
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.01
}

fn assert_at(cmds: &[PathCmd], x: f32, y: f32) {
    let (ex, ey) = end(cmds);
    assert!(
        near(ex, x) && near(ey, y),
        "ends at {ex},{ey} rather than {x},{y}"
    );
}

#[test]
fn a_move_and_a_line() {
    let p = parse_path("M 10 20 L 30 40");
    assert_eq!(p.len(), 2, "a move and one curve, since a line is a curve");
    assert_eq!(p[0], PathCmd::Move { x: 10.0, y: 20.0 });
    // Controls at the thirds, which is what makes this interpolate against any
    // other curve.
    let PathCmd::Curve {
        x1,
        y1,
        x2,
        y2,
        x,
        y,
    } = p[1]
    else {
        panic!("a line is a curve, got {:?}", p[1])
    };
    assert!(near(x1, 16.6667) && near(y1, 26.6667), "{x1},{y1}");
    assert!(near(x2, 23.3333) && near(y2, 33.3333), "{x2},{y2}");
    assert!(near(x, 30.0) && near(y, 40.0), "{x},{y}");
}

/// Relative commands measure from where the pen is, and every one of them
/// moves it.
#[test]
fn relative_commands_accumulate() {
    assert_at(&parse_path("m 10 10 l 5 5 l 5 5"), 20.0, 20.0);
    assert_at(&parse_path("M 0 0 h 10 v 10 h -4"), 6.0, 10.0);
}

/// A command applies to every run of arguments that follows it, which is how
/// nearly all real path data is written.
#[test]
fn a_command_repeats_over_its_arguments() {
    let p = parse_path("M 0 0 L 1 1 2 2 3 3");
    assert_eq!(p.len(), 4, "one move and three lines from one L");
    assert_at(&p, 3.0, 3.0);
}

/// The exception to the repeat rule, and the one that quietly ruins polygons:
/// a second pair of numbers after `M` is a *line*, not another move.
#[test]
fn a_repeated_move_is_a_line() {
    let p = parse_path("M 0 0 10 0 10 10");
    assert_eq!(p.len(), 3);
    assert!(
        matches!(p[1], PathCmd::Curve { .. }) && matches!(p[2], PathCmd::Curve { .. }),
        "the second and third pairs draw, they do not jump: {p:?}"
    );
}

/// `Z` returns the pen to where the subpath began, so what follows it is
/// measured from there and not from the last point drawn.
#[test]
fn close_returns_the_pen_to_the_subpath_start() {
    let p = parse_path("M 10 10 L 50 50 Z l 5 5");
    assert_eq!(p[2], PathCmd::Close);
    assert_at(&p, 15.0, 15.0);
}

/// Numbers run together with no separator, which is legal and is what a
/// minifier emits.
#[test]
fn numbers_need_no_separators() {
    assert_at(&parse_path("M0 0L1-2"), 1.0, -2.0);
    assert_at(&parse_path("M0 0L.5.5"), 0.5, 0.5);
    assert_at(&parse_path("M0,0L1e2,1.5E1"), 100.0, 15.0);
    assert_at(&parse_path("M0 0L10 10"), 10.0, 10.0);
}

/// A quadratic raised to a cubic is exact, so its midpoint is the quadratic's
/// midpoint and not an approximation of it.
#[test]
fn a_quadratic_becomes_the_same_curve() {
    let p = parse_path("M 0 0 Q 50 100 100 0");
    let PathCmd::Curve {
        x1,
        y1,
        x2,
        y2,
        x,
        y,
    } = p[1]
    else {
        panic!("expected a curve, got {:?}", p[1])
    };
    // Controls two thirds of the way to the quadratic's single control.
    assert!(near(x1, 33.3333) && near(y1, 66.6667), "{x1},{y1}");
    assert!(near(x2, 66.6667) && near(y2, 66.6667), "{x2},{y2}");
    assert!(near(x, 100.0) && near(y, 0.0));
}

/// `S` reflects the previous cubic's second control about the current point.
#[test]
fn a_smooth_cubic_reflects_the_previous_control() {
    let p = parse_path("M 0 0 C 10 10 20 10 30 0 S 50 -10 60 0");
    let PathCmd::Curve { x1, y1, .. } = p[2] else {
        panic!("expected a curve")
    };
    // Previous second control was (20, 10) and the pen is at (30, 0), so the
    // reflection is (40, -10).
    assert!(near(x1, 40.0) && near(y1, -10.0), "{x1},{y1}");
}

/// With nothing to reflect, the control coincides with the current point. The
/// separate tracking matters: a `T` after a `C` has no *quadratic* control to
/// reflect even though a cubic one exists.
#[test]
fn a_smooth_command_with_nothing_behind_it_uses_the_current_point() {
    let p = parse_path("M 10 10 S 30 30 40 10");
    let PathCmd::Curve { x1, y1, .. } = p[1] else {
        panic!("expected a curve")
    };
    assert!(near(x1, 10.0) && near(y1, 10.0), "{x1},{y1}");

    let p = parse_path("M 0 0 C 10 10 20 10 30 0 T 60 0");
    let PathCmd::Curve { x1, y1, .. } = p[2] else {
        panic!("expected a curve")
    };
    // Raised from a quadratic whose control is the current point (30, 0).
    assert!(near(x1, 30.0) && near(y1, 0.0), "{x1},{y1}");
}

/// An arc's flags are single characters and need no separator, so `011` is
/// two flags and a coordinate rather than the number eleven.
#[test]
fn arc_flags_run_together_with_the_coordinate() {
    let p = parse_path("M 0 0 a 10 10 0 011 1");
    assert_at(&p, 1.0, 1.0);
    assert!(p.len() > 1, "the arc drew something: {p:?}");
}

/// A half circle by arc really is a half circle: sampling the chain of cubics
/// at its own endpoints has to stay on the ellipse.
#[test]
fn an_arc_lands_on_its_ellipse() {
    // From (0,0) to (100,0), radius 50, sweeping over the top.
    let p = parse_path("M 0 0 A 50 50 0 0 1 100 0");
    assert_at(&p, 100.0, 0.0);
    // Cut into quarter turns, so a half turn is two cubics after the move.
    assert_eq!(p.len(), 3, "a move and two quarter-turn cubics: {p:?}");
    // Every point the chain passes through is 50 from the centre (50, 0).
    for c in &p[1..] {
        let PathCmd::Curve { x, y, .. } = *c else {
            panic!("expected curves")
        };
        let r = ((x - 50.0).powi(2) + y.powi(2)).sqrt();
        assert!(near(r, 50.0), "endpoint {x},{y} is {r} from the centre");
    }
}

/// Radii too small to span the two endpoints are scaled up until they exactly
/// do, rather than the arc vanishing. Hand-written path data gets this wrong
/// constantly.
#[test]
fn radii_too_small_are_scaled_up_rather_than_dropped() {
    let p = parse_path("M 0 0 A 1 1 0 0 1 100 0");
    assert!(p.len() > 1, "the arc still drew: {p:?}");
    assert_at(&p, 100.0, 0.0);
}

/// An arc that goes nowhere draws nothing. It is not a zero-length line, and
/// emitting one would leave a stray round cap on screen.
#[test]
fn an_arc_to_where_it_already_is_draws_nothing() {
    let p = parse_path("M 10 10 A 50 50 0 1 1 10 10");
    assert_eq!(p.len(), 1, "only the move survives: {p:?}");
}

/// Malformed data keeps what was read. A path with a typo three quarters along
/// still draws its first three quarters, which is what shows the author where
/// the typo is.
#[test]
fn a_broken_path_keeps_what_came_before_it() {
    let p = parse_path("M 0 0 L 10 10 L 20");
    assert_eq!(p.len(), 2, "the complete commands survive: {p:?}");
    assert_at(&p, 10.0, 10.0);

    assert!(parse_path("").is_empty());
    assert!(parse_path("not a path").is_empty());
    assert!(parse_path("10 20 30").is_empty(), "numbers with no command");
}

/// Two paths with the same command sequence interpolate point by point.
#[test]
fn matching_shapes_interpolate() {
    let a = parse_path("M 0 0 L 10 0");
    let b = parse_path("M 0 0 L 20 0");
    let mid = lerp(&a, &b, 0.5).expect("same sequence");
    assert_at(&mid, 15.0, 0.0);

    // And the ends are the ends, not something near them.
    assert_eq!(lerp(&a, &b, 0.0).unwrap(), a);
    assert_eq!(lerp(&a, &b, 1.0).unwrap(), b);
}

/// Sequences that disagree do not interpolate, and say so rather than
/// guessing a correspondence. Tier 1's rule for every other value: same shape
/// interpolates, anything else jumps.
#[test]
fn mismatched_shapes_refuse_to_interpolate() {
    let two = parse_path("M 0 0 L 10 0");
    let three = parse_path("M 0 0 L 10 0 L 10 10");
    assert!(lerp(&two, &three, 0.5).is_none(), "different lengths");

    let closed = parse_path("M 0 0 L 10 0 Z");
    let open = parse_path("M 0 0 L 10 0 L 5 5");
    assert!(
        lerp(&closed, &open, 0.5).is_none(),
        "a close does not become a curve"
    );
}

/// A rectangle and a circle morph into one another, which is the case the
/// all-cubics normalisation exists for. Written with the same command count,
/// they interpolate; there is no resampling and none is attempted.
#[test]
fn a_square_can_become_a_circle() {
    let square = parse_path("M 50 0 L 100 0 L 100 100 L 0 100 L 0 0 Z");
    let circle = parse_path("M 50 0 A 50 50 0 0 1 100 50 A 50 50 0 0 1 50 100 A 50 50 0 0 1 0 50 A 50 50 0 0 1 50 0 Z");
    assert_eq!(
        square.len(),
        circle.len(),
        "four sides against four quarter turns, plus the move and the close"
    );
    assert!(lerp(&square, &circle, 0.5).is_some(), "and they interpolate");
}

/// The bounding box is the control hull, so it is never smaller than the
/// drawing. That is the direction to be wrong in: a few empty pixels rather
/// than a clipped shape.
#[test]
fn bounds_cover_the_whole_drawing() {
    let p = PathContent::parse("M 0 0 C 0 100 100 100 100 0");
    let (x0, y0, x1, y1) = p.bounds().expect("has geometry");
    assert!(near(x0, 0.0) && near(y0, 0.0));
    assert!(near(x1, 100.0) && near(y1, 100.0));
    assert!(PathContent::parse("").bounds().is_none(), "nothing to bound");
}

/// `d` is kept as written, because it is the identity a later build compares
/// against and string equality is cheaper than walking two command lists.
#[test]
fn the_source_is_kept_alongside_the_commands() {
    let p = PathContent::parse("M 0 0 L 1 1");
    assert_eq!(p.d, "M 0 0 L 1 1");
    assert_eq!(p.commands.len(), 2);
}
