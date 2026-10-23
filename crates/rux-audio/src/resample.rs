//! A sound's rate made the output's, by straight-line interpolation between
//! neighbouring frames.
//!
//! **Linear, and so not the best a resampler can do**: going from 44.1 kHz
//! to 48 kHz it softens the very top of the range a little. It costs next to
//! nothing and runs a stream's chunks one after another with no seam, which
//! is what a first version needs; a windowed-sinc one can replace it behind
//! the same two calls.

pub(crate) struct Resampler {
    channels: usize,
    /// Input frames per output frame.
    step: f64,
    /// Where the next output frame falls, in input frames counted from the
    /// kept frame `last` (0) when there is one, or the chunk's first.
    pos: f64,
    /// The previous chunk's last frame, which the next interpolates from.
    last: Option<Vec<f32>>,
}

impl Resampler {
    pub fn new(from: u32, to: u32, channels: usize) -> Self {
        Resampler { channels: channels.max(1), step: from as f64 / to.max(1) as f64, pos: 0.0, last: None }
    }

    /// Start again, as after a seek: nothing carried over.
    pub fn reset(&mut self) {
        self.pos = 0.0;
        self.last = None;
    }

    /// `input`'s frames at the new rate, appended to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        let ch = self.channels;
        if (self.step - 1.0).abs() < 1e-9 {
            out.extend_from_slice(input);
            return;
        }
        let frames = input.len() / ch;
        if frames == 0 {
            return;
        }
        // Frame `i` of the input with the kept one in front of it.
        let offset = usize::from(self.last.is_some());
        let total = frames + offset;
        let last = self.last.take();
        let at = |i: usize, c: usize| -> f32 {
            if i < offset {
                last.as_ref().map_or(0.0, |l| l[c])
            } else {
                input[(i - offset) * ch + c]
            }
        };
        while self.pos + 1.0 < total as f64 {
            let i = self.pos.floor() as usize;
            let t = (self.pos - i as f64) as f32;
            for c in 0..ch {
                let a = at(i, c);
                let b = at(i + 1, c);
                out.push(a + (b - a) * t);
            }
            self.pos += self.step;
        }
        // The last frame becomes frame 0 of the next chunk.
        self.pos -= (total - 1) as f64;
        self.last = Some(input[(frames - 1) * ch..frames * ch].to_vec());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_length_follows_the_rates() {
        let input: Vec<f32> = (0..44_100).map(|i| (i as f32 * 0.01).sin()).collect();
        let mut out = Vec::new();
        Resampler::new(44_100, 48_000, 1).process(&input, &mut out);
        assert!((out.len() as i64 - 48_000).abs() <= 2, "{}", out.len());
    }

    #[test]
    fn chunks_join_with_no_seam() {
        let input: Vec<f32> = (0..2_000).flat_map(|i| [(i as f32 * 0.02).sin(), (i as f32 * 0.03).cos()]).collect();
        let mut whole = Vec::new();
        Resampler::new(44_100, 48_000, 2).process(&input, &mut whole);
        let mut parts = Vec::new();
        let mut r = Resampler::new(44_100, 48_000, 2);
        for chunk in input.chunks(2 * 137) {
            r.process(chunk, &mut parts);
        }
        assert_eq!(whole.len(), parts.len());
        for (a, b) in whole.iter().zip(&parts) {
            assert!((a - b).abs() < 1e-5, "{a} {b}");
        }
    }

    #[test]
    fn the_same_rate_is_the_same_samples() {
        let input = [0.1, 0.2, 0.3, 0.4];
        let mut out = Vec::new();
        Resampler::new(48_000, 48_000, 2).process(&input, &mut out);
        assert_eq!(out, input);
    }
}
