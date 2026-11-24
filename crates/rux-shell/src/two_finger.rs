//! Two fingers as one gesture: how far apart they are (a pinch), which way
//! the line between them points (a rotate), and where it sits (a pan).
//!
//! Pure geometry, so it is tested without a window. The shell feeds it the two
//! fingers' positions in logical pixels and hands the answer to `@gesturechange`.
//! It follows the same two fingers throughout: the pair is fixed when the
//! second one lands, by their ids, and a finger that swaps sides with the other
//! turns the line by half a turn, which is what it did.

/// What the two fingers have done since they landed, and since the last report.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Update {
    /// The distance between them over what it was: 1 when unchanged, 2 when
    /// twice as far apart.
    pub scale: f32,
    /// Degrees the line between them has turned, clockwise on the screen,
    /// counted across the half-turn (it keeps adding past 180).
    pub rotation: f32,
    /// `scale` over the previous report's `scale`.
    pub delta_scale: f32,
    /// `rotation` less the previous report's.
    pub delta_rotation: f32,
    /// The middle between the fingers.
    pub center: (f32, f32),
    /// How far the middle moved since the previous report.
    pub delta: (f32, f32),
}

/// Two fingers followed from when the second one landed.
#[derive(Clone, Copy, Debug)]
pub struct TwoFinger {
    start_distance: f32,
    last_angle: f32,
    rotation: f32,
    last_scale: f32,
    last_center: (f32, f32),
}

fn centre(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    (b.0 - a.0).hypot(b.1 - a.1)
}

/// Screen coordinates have y pointing down, so a growing angle turns clockwise.
fn angle(a: (f32, f32), b: (f32, f32)) -> f32 {
    (b.1 - a.1).atan2(b.0 - a.0)
}

impl TwoFinger {
    /// The fingers have just landed at `a` and `b`. A distance under a pixel is
    /// taken as one, so two fingers on the same spot do not divide by nothing.
    pub fn start(a: (f32, f32), b: (f32, f32)) -> TwoFinger {
        TwoFinger {
            start_distance: distance(a, b).max(1.0),
            last_angle: angle(a, b),
            rotation: 0.0,
            last_scale: 1.0,
            last_center: centre(a, b),
        }
    }

    /// The fingers are now at `a` and `b`, the same two as at the start.
    pub fn update(&mut self, a: (f32, f32), b: (f32, f32)) -> Update {
        let scale = distance(a, b) / self.start_distance;
        let now = angle(a, b);
        // The turn since the last report, taken the short way round, so a long
        // turn keeps adding up instead of jumping at the half-turn.
        let mut turn = now - self.last_angle;
        while turn > std::f32::consts::PI {
            turn -= std::f32::consts::TAU;
        }
        while turn <= -std::f32::consts::PI {
            turn += std::f32::consts::TAU;
        }
        self.last_angle = now;
        self.rotation += turn.to_degrees();
        let center = centre(a, b);
        let update = Update {
            scale,
            rotation: self.rotation,
            delta_scale: if self.last_scale > 0.0 { scale / self.last_scale } else { 1.0 },
            delta_rotation: turn.to_degrees(),
            center,
            delta: (center.0 - self.last_center.0, center.1 - self.last_center.1),
        };
        self.last_scale = scale;
        self.last_center = center;
        update
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn spreading_the_fingers_scales_and_does_not_rotate() {
        let mut g = TwoFinger::start((100.0, 100.0), (200.0, 100.0));
        let u = g.update((50.0, 100.0), (250.0, 100.0));
        assert!(near(u.scale, 2.0) && near(u.rotation, 0.0), "{u:?}");
        assert!(near(u.delta_scale, 2.0) && near(u.delta_rotation, 0.0));
        assert_eq!(u.center, (150.0, 100.0));
        assert_eq!(u.delta, (0.0, 0.0), "the middle did not move");
        let u = g.update((75.0, 100.0), (225.0, 100.0));
        assert!(near(u.scale, 1.5) && near(u.delta_scale, 0.75), "{u:?}");
    }

    #[test]
    fn turning_the_line_rotates_clockwise_and_does_not_scale() {
        // The second finger goes from the right of the first to below it: a
        // quarter turn clockwise on a screen whose y points down.
        let mut g = TwoFinger::start((100.0, 100.0), (200.0, 100.0));
        let u = g.update((100.0, 100.0), (100.0, 200.0));
        assert!(near(u.rotation, 90.0) && near(u.scale, 1.0), "{u:?}");
        assert!(near(u.delta_rotation, 90.0));
    }

    #[test]
    fn a_long_turn_keeps_counting_past_the_half_turn() {
        let mut g = TwoFinger::start((0.0, 0.0), (100.0, 0.0));
        let mut last = 0.0;
        // Every 30 degrees clockwise, a full turn and a half.
        for step in 1..=18 {
            let a = (step as f32 * 30.0).to_radians();
            let u = g.update((0.0, 0.0), (100.0 * a.cos(), 100.0 * a.sin()));
            assert!(near(u.delta_rotation, 30.0), "step {step}: {u:?}");
            last = u.rotation;
        }
        assert!(near(last, 540.0), "{last}");
        // And back the other way.
        let u = g.update((0.0, 0.0), (100.0 * 510f32.to_radians().cos(), 100.0 * 510f32.to_radians().sin()));
        assert!(near(u.delta_rotation, -30.0) && near(u.rotation, 510.0), "{u:?}");
    }

    #[test]
    fn moving_both_fingers_together_pans_and_changes_nothing_else() {
        let mut g = TwoFinger::start((100.0, 100.0), (200.0, 100.0));
        let u = g.update((130.0, 110.0), (230.0, 110.0));
        assert!(near(u.scale, 1.0) && near(u.rotation, 0.0), "{u:?}");
        assert_eq!(u.delta, (30.0, 10.0));
        assert_eq!(u.center, (180.0, 110.0));
    }

    #[test]
    fn two_fingers_on_one_spot_do_not_divide_by_nothing() {
        let mut g = TwoFinger::start((50.0, 50.0), (50.0, 50.0));
        let u = g.update((40.0, 50.0), (60.0, 50.0));
        assert!(u.scale.is_finite() && near(u.scale, 20.0), "{u:?}");
        assert!(u.rotation.is_finite());
    }
}
