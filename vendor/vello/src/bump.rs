// Rux: this module is Rux's, not vello's. See `vendor/README.md`.

//! How long the bump-allocated buffers are.
//!
//! vello sizes its scratch buffers at fixed lengths, about 150 MB whatever
//! the scene, and with the frames a window keeps in flight a Rux window
//! held about 475 MB of GPU memory to draw text and boxes (Tracer's
//! AW-022). The GPU already counts what a frame needed of each, in the
//! `BumpAllocators` it writes. So each buffer starts small; after the
//! coarse stages the counts are read back, and a frame that ran out is run
//! again with what it needed and a quarter to spare. Lengths a long run of
//! frames no longer needs come down again.

use vello_encoding::{BumpAllocators, BumpSizes};

/// Where every buffer starts, and the least it shrinks to: a few hundred KB
/// in all, enough for a small window of text without growing.
const FIRST: BumpSizes = BumpSizes {
    bin_data: 1 << 14,
    tiles: 1 << 15,
    lines: 1 << 15,
    seg_counts: 1 << 15,
    segments: 1 << 15,
    blend_spill: 1 << 12,
    ptcl: 1 << 17,
};

/// How many frames drawn before lengths far above what they needed shrink.
const WINDOW: u32 = 600;

/// The bits of `BumpAllocators::failed` that name a stage, from vello's
/// `shader/shared/bump.wgsl`.
const STAGE_BINNING: u32 = 0x1;

/// After this many runs of one frame, vello's own lengths, and after one
/// more the frame is drawn as it is.
const TRIES: u32 = 6;

#[derive(Debug)]
pub(crate) struct Sizing {
    now: BumpSizes,
    /// The most any frame needed since the last shrink.
    high: BumpSizes,
    frames: u32,
}

impl Default for Sizing {
    fn default() -> Self {
        Self { now: FIRST, high: zero(), frames: 0 }
    }
}

fn zero() -> BumpSizes {
    BumpSizes { bin_data: 0, tiles: 0, lines: 0, seg_counts: 0, segments: 0, blend_spill: 0, ptcl: 0 }
}

/// `n` and a quarter more, rounded up to a power of two so lengths repeat
/// and the engine's pool reuses them.
fn room(n: u32) -> u32 {
    n.saturating_add(n / 4).max(1).next_power_of_two()
}

/// What `u` says a frame needed, buffer by buffer: the counted part of
/// each. The part of `bin_data` and `ptcl` the GPU does not count (the draw
/// objects' info, every tile's own words) is added by
/// `RenderConfig::new_sized`, which knows the scene and the frame's size.
fn needed(u: &BumpAllocators) -> BumpSizes {
    BumpSizes {
        bin_data: u.binning,
        tiles: u.tile,
        lines: u.lines,
        seg_counts: u.seg_counts,
        segments: u.segments,
        blend_spill: u.blend,
        ptcl: u.ptcl,
    }
}

fn each(a: &mut BumpSizes, b: &BumpSizes, mut f: impl FnMut(&mut u32, u32)) {
    f(&mut a.bin_data, b.bin_data);
    f(&mut a.tiles, b.tiles);
    f(&mut a.lines, b.lines);
    f(&mut a.seg_counts, b.seg_counts);
    f(&mut a.segments, b.segments);
    f(&mut a.blend_spill, b.blend_spill);
    f(&mut a.ptcl, b.ptcl);
}

impl Sizing {
    pub(crate) fn sizes(&self) -> BumpSizes {
        self.now
    }

    /// What the coarse stages of a frame used, on its `tries`th run. True
    /// when a buffer ran out and has grown, for the frame to run again.
    pub(crate) fn saw(&mut self, u: &BumpAllocators, tries: u32) -> bool {
        let want = needed(u);
        each(&mut self.high, &want, |h, w| *h = (*h).max(w));
        let mut short = false;
        each(&mut self.now.clone(), &want, |n, w| short |= w > *n);
        if u.failed == 0 && !short {
            self.frames += 1;
            return false;
        }
        if tries > TRIES {
            return false;
        }
        if tries == TRIES {
            each(&mut self.now, &BumpSizes::default(), |n, d| *n = (*n).max(d));
            return true;
        }
        let before = self.now;
        each(&mut self.now, &want, |n, w| {
            if w > *n {
                *n = room(w);
            }
        });
        if u.failed & STAGE_BINNING != 0 {
            self.now.bin_data = self.now.bin_data.saturating_mul(2);
        }
        // Failed with every count inside its buffer: a stage this does not
        // know how to read. Everything doubles.
        if self.now == before {
            each(&mut self.now, &zero(), |n, _| *n = n.saturating_mul(2));
        }
        true
    }

    /// Once a frame is drawn. True when lengths came down, every `WINDOW`
    /// frames, where one is four times what the most any frame needed in
    /// that time and a quarter to spare comes to.
    pub(crate) fn settled(&mut self) -> bool {
        if self.frames < WINDOW {
            return false;
        }
        self.frames = 0;
        let before = self.now;
        each(&mut self.now, &self.high.clone(), |n, h| {
            let enough = room(h).max(1);
            if *n >= enough.saturating_mul(4) {
                *n = enough.saturating_mul(2);
            }
        });
        each(&mut self.now, &FIRST, |n, f| *n = (*n).max(f));
        self.high = zero();
        self.now != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn used(f: impl FnOnce(&mut BumpAllocators)) -> BumpAllocators {
        let mut u = BumpAllocators::default();
        f(&mut u);
        u
    }

    #[test]
    fn a_frame_that_fits_runs_once() {
        let mut s = Sizing::default();
        assert!(!s.saw(&used(|u| u.lines = 100), 1));
        assert_eq!(s.sizes(), FIRST);
    }

    #[test]
    fn a_buffer_that_ran_out_grows_to_what_was_needed_and_a_quarter() {
        let mut s = Sizing::default();
        assert!(s.saw(&used(|u| (u.failed, u.lines) = (0x4, 100_000)), 1));
        assert_eq!(s.sizes().lines, 131_072, "125,000 up to a power of two");
        assert_eq!(s.sizes().tiles, FIRST.tiles, "the others as they were");
        assert!(!s.saw(&used(|u| u.lines = 100_000), 2), "then it fits");
    }

    #[test]
    fn binning_doubles_on_its_failure_and_an_unread_failure_doubles_all() {
        let mut s = Sizing::default();
        assert!(s.saw(&used(|u| u.failed = STAGE_BINNING), 1));
        assert_eq!(s.sizes().bin_data, FIRST.bin_data * 2);
        let mut s = Sizing::default();
        assert!(s.saw(&used(|u| u.failed = 0x10), 1));
        assert_eq!(s.sizes().ptcl, FIRST.ptcl * 2);
    }

    #[test]
    fn after_enough_tries_vellos_own_lengths_and_then_the_frame_as_it_is() {
        let mut s = Sizing::default();
        assert!(s.saw(&used(|u| u.failed = 0x10), TRIES));
        assert_eq!(s.sizes().ptcl, BumpSizes::default().ptcl);
        assert!(!s.saw(&used(|u| u.failed = 0x10), TRIES + 1));
    }

    #[test]
    fn lengths_no_frame_needs_come_down_after_a_while() {
        let mut s = Sizing::default();
        s.saw(&used(|u| (u.failed, u.segments) = (0x10, 1 << 20)), 1);
        assert!(s.sizes().segments >= 1 << 20);
        // The frame that needed it counts in its own window, so it comes
        // down at the end of the next.
        for _ in 0..2 * WINDOW {
            assert!(!s.saw(&used(|u| u.segments = 100), 1));
            s.settled();
        }
        assert_eq!(s.sizes().segments, FIRST.segments, "back to where it started");
    }
}
