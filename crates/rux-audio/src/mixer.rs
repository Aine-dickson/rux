//! Every playing sound summed into the output's frames.
//!
//! Runs on the audio thread, so it never waits on anything but the sounds'
//! own small locks: a streamed sound's samples arrive through a channel its
//! reading thread fills ahead, and one that falls behind plays silence for
//! that moment rather than stalling the output.

use std::sync::mpsc::{self, Receiver, Sender, SyncSender, TryRecvError};
use std::sync::{Arc, Weak};

use crate::decode::Decoding;
use crate::resample::Resampler;
use crate::{AudioError, Input, Pcm, Shared, Source};

/// The sounds playing into one output.
pub struct Mixer {
    rate: u32,
    channels: usize,
    voices: Vec<Voice>,
    /// Frames played since the sounds' readers were last told.
    since_notify: usize,
    /// Whether a sound played in the last [`Mixer::mix`], to tell when the
    /// last one stops.
    was_playing: bool,
}

/// A sound the mixer knows, and where its samples come from.
pub(crate) struct Voice {
    id: u64,
    sound: Weak<Shared>,
    feed: Feed,
}

enum Feed {
    /// Decoded whole at the output's rate; `frame` is where it is.
    Whole { pcm: Arc<Pcm>, frame: usize },
    Stream(StreamFeed),
}

/// A sound read ahead by its own thread.
struct StreamFeed {
    rx: Receiver<Chunk>,
    ctl: Sender<Ctl>,
    channels: usize,
    /// Which seek the samples belong to: those of an earlier one are dropped.
    gen: u64,
    cur: Vec<f32>,
    offset: usize,
    /// At its end, waiting for a seek or a `play` to start it again.
    ended: bool,
}

pub(crate) enum Chunk {
    Data { gen: u64, samples: Vec<f32> },
    /// It looped back to the start.
    Wrapped { gen: u64 },
    End { gen: u64 },
    Failed { gen: u64, error: AudioError },
}

pub(crate) enum Ctl {
    Seek { secs: f64, gen: u64 },
}

/// Chunks read ahead of a streamed sound: a few tenths of a second.
const READ_AHEAD: usize = 16;

/// What one pull from a feed gave.
enum Pulled {
    Frames(usize),
    Wrapped,
    End,
    Starved,
    Failed(AudioError),
}

impl Voice {
    pub(crate) fn new(shared: &Arc<Shared>, rate: u32) -> Result<Voice, AudioError> {
        let feed = match &shared.source {
            Source::Whole(pcm) => {
                let pcm = if pcm.rate == rate || pcm.rate == 0 {
                    Arc::clone(pcm)
                } else {
                    let mut samples = Vec::with_capacity(pcm.samples.len() * rate as usize / pcm.rate as usize + 8);
                    Resampler::new(pcm.rate, rate, pcm.channels).process(&pcm.samples, &mut samples);
                    Arc::new(Pcm { rate, channels: pcm.channels, samples })
                };
                Feed::Whole { pcm, frame: 0 }
            }
            Source::Stream(input) => Feed::Stream(stream(input.clone(), Arc::downgrade(shared), rate)?),
        };
        let mut feed = feed;
        // Made again for a sound part way through (a new output, or one
        // opened again after the app was put away): from where it is.
        let st = shared.state();
        if st.current > 0.0 && !st.at_end {
            feed.seek(st.current, rate);
        }
        drop(st);
        Ok(Voice { id: shared.id, sound: Arc::downgrade(shared), feed })
    }
}

/// Start the thread that reads `input` ahead, at `rate`.
fn stream(input: Input, sound: Weak<Shared>, rate: u32) -> Result<StreamFeed, AudioError> {
    let mut d = Decoding::open(&input)?;
    let channels = d.channels.max(1);
    let (tx, rx) = mpsc::sync_channel::<Chunk>(READ_AHEAD);
    let (ctl, ctl_rx) = mpsc::channel::<Ctl>();
    std::thread::Builder::new()
        .name("rux-audio-read".into())
        .spawn(move || read_ahead(&mut d, rate, &tx, &ctl_rx, &sound))
        .map_err(|e| AudioError::new("io", format!("cannot start reading the sound: {e}")))?;
    Ok(StreamFeed { rx, ctl, channels, gen: 0, cur: Vec::new(), offset: 0, ended: false })
}

/// The reading thread: decode, resample, send, until the sound is dropped.
fn read_ahead(d: &mut Decoding, rate: u32, tx: &SyncSender<Chunk>, ctl: &Receiver<Ctl>, sound: &Weak<Shared>) {
    let mut rs = Resampler::new(d.rate, rate, d.channels.max(1));
    let mut gen = 0;
    let seek = |d: &mut Decoding, rs: &mut Resampler, secs: f64| {
        rs.reset();
        d.seek(secs)
    };
    loop {
        // A seek asked for since the last chunk.
        loop {
            match ctl.try_recv() {
                Ok(Ctl::Seek { secs, gen: g }) => {
                    gen = g;
                    if let Err(error) = seek(d, &mut rs, secs) {
                        let _ = tx.send(Chunk::Failed { gen, error });
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        let next = match d.next() {
            Ok(Some(samples)) => {
                let mut out = Vec::with_capacity(samples.len() + 8);
                rs.process(&samples, &mut out);
                Chunk::Data { gen, samples: out }
            }
            Ok(None) => {
                let looping = sound.upgrade().is_some_and(|s| s.state().looping);
                if looping && seek(d, &mut rs, 0.0).is_ok() {
                    Chunk::Wrapped { gen }
                } else {
                    Chunk::End { gen }
                }
            }
            Err(error) => Chunk::Failed { gen, error },
        };
        let waits = matches!(next, Chunk::End { .. } | Chunk::Failed { .. });
        if tx.send(next).is_err() {
            return;
        }
        if waits {
            // Nothing more to read until it is sought, `stop` and a replay
            // included.
            match ctl.recv() {
                Ok(Ctl::Seek { secs, gen: g }) => {
                    gen = g;
                    if let Err(error) = seek(d, &mut rs, secs) {
                        let _ = tx.send(Chunk::Failed { gen, error });
                    }
                }
                Err(_) => return,
            }
        }
    }
}

impl Feed {
    fn seek(&mut self, secs: f64, rate: u32) {
        match self {
            Feed::Whole { pcm, frame } => {
                let frames = pcm.samples.len() / pcm.channels.max(1);
                *frame = ((secs * rate as f64) as usize).min(frames);
            }
            Feed::Stream(s) => {
                s.gen += 1;
                let _ = s.ctl.send(Ctl::Seek { secs, gen: s.gen });
                s.cur.clear();
                s.offset = 0;
                s.ended = false;
                // Empty what was read ahead, which also frees the reader if it
                // was waiting for room.
                while s.rx.try_recv().is_ok() {}
            }
        }
    }

    fn channels(&self) -> usize {
        match self {
            Feed::Whole { pcm, .. } => pcm.channels.max(1),
            Feed::Stream(s) => s.channels,
        }
    }

    /// Up to `max` frames, added into `out` (the output's `och` channels per
    /// frame) at `volume`.
    fn pull(&mut self, out: &mut [f32], och: usize, max: usize, volume: f32) -> Pulled {
        let sch = self.channels();
        match self {
            Feed::Whole { pcm, frame } => {
                let total = pcm.samples.len() / sch;
                if *frame >= total {
                    return Pulled::End;
                }
                let n = max.min(total - *frame);
                add(&pcm.samples[*frame * sch..(*frame + n) * sch], sch, out, och, volume);
                *frame += n;
                Pulled::Frames(n)
            }
            Feed::Stream(s) => {
                if s.ended {
                    return Pulled::End;
                }
                if s.offset >= s.cur.len() {
                    loop {
                        match s.rx.try_recv() {
                            Ok(Chunk::Data { gen, samples }) if gen == s.gen => {
                                s.cur = samples;
                                s.offset = 0;
                                break;
                            }
                            Ok(Chunk::Wrapped { gen }) if gen == s.gen => return Pulled::Wrapped,
                            Ok(Chunk::End { gen }) if gen == s.gen => {
                                s.ended = true;
                                return Pulled::End;
                            }
                            Ok(Chunk::Failed { gen, error }) if gen == s.gen => {
                                s.ended = true;
                                return Pulled::Failed(error);
                            }
                            // From before a seek.
                            Ok(_) => continue,
                            Err(_) => return Pulled::Starved,
                        }
                    }
                }
                let left = (s.cur.len() - s.offset) / sch;
                let n = max.min(left);
                add(&s.cur[s.offset..s.offset + n * sch], sch, out, och, volume);
                s.offset += n * sch;
                Pulled::Frames(n)
            }
        }
    }
}

/// `src`'s frames (`sch` channels) added into `out`'s (`och`): the same
/// channels as they are, mono to every channel, and everything averaged into
/// mono.
fn add(src: &[f32], sch: usize, out: &mut [f32], och: usize, volume: f32) {
    for (i, frame) in src.chunks_exact(sch).enumerate() {
        let o = &mut out[i * och..(i + 1) * och];
        if sch == och {
            for (d, s) in o.iter_mut().zip(frame) {
                *d += s * volume;
            }
        } else if sch == 1 {
            for d in o.iter_mut() {
                *d += frame[0] * volume;
            }
        } else if och == 1 {
            o[0] += frame.iter().sum::<f32>() / sch as f32 * volume;
        } else {
            for (d, s) in o.iter_mut().zip(frame) {
                *d += s * volume;
            }
        }
    }
}

impl Mixer {
    pub fn new(rate: u32, channels: usize) -> Self {
        Mixer { rate, channels: channels.max(1), voices: Vec::new(), since_notify: 0, was_playing: false }
    }

    pub(crate) fn has(&self, id: u64) -> bool {
        self.voices.iter().any(|v| v.id == id)
    }

    pub(crate) fn add(&mut self, v: Voice) {
        self.voices.push(v);
    }

    /// Every sound it knows that someone still holds.
    pub(crate) fn sounds(&self) -> Vec<Arc<Shared>> {
        self.voices.iter().filter_map(|v| v.sound.upgrade()).collect()
    }

    pub(crate) fn any_playing(&self) -> bool {
        self.voices.iter().filter_map(|v| v.sound.upgrade()).any(|s| s.state().playing)
    }

    /// Fill `out`, interleaved at the output's channels, with every playing
    /// sound summed.
    pub fn mix(&mut self, out: &mut [f32]) {
        out.fill(0.0);
        let och = self.channels;
        let frames = out.len() / och;
        let rate = self.rate;
        let mut any_playing = false;
        let mut moved = false;
        // A sound nobody holds any more is gone.
        self.voices.retain(|v| v.sound.strong_count() > 0);
        for v in &mut self.voices {
            let Some(sound) = v.sound.upgrade() else { continue };
            let mut st = sound.state();
            if let Some(secs) = st.seek.take() {
                v.feed.seek(secs, rate);
            }
            if !st.playing {
                continue;
            }
            any_playing = true;
            let volume = st.volume;
            let mut done = 0;
            while done < frames {
                match v.feed.pull(&mut out[done * och..], och, frames - done, volume) {
                    Pulled::Frames(n) => {
                        done += n;
                        st.current += n as f64 / rate as f64;
                    }
                    Pulled::Wrapped => st.current = 0.0,
                    Pulled::End if st.looping => {
                        // A whole sound loops here; a stream's reader loops
                        // itself, and ending means it was not asked to.
                        if let Feed::Whole { frame, .. } = &mut v.feed {
                            *frame = 0;
                            st.current = 0.0;
                        } else {
                            v.feed.seek(0.0, rate);
                            st.current = 0.0;
                            break;
                        }
                    }
                    Pulled::End => {
                        st.playing = false;
                        st.at_end = true;
                        st.ended += 1;
                        if let Some(d) = sound.duration {
                            st.current = d;
                        }
                        v.feed.seek(0.0, rate);
                        moved = true;
                        break;
                    }
                    Pulled::Failed(e) => {
                        st.playing = false;
                        st.error = Some(e);
                        moved = true;
                        break;
                    }
                    Pulled::Starved => break,
                }
            }
        }
        if any_playing {
            self.since_notify += frames;
        }
        // Playing at the start of this mix is not playing at its end when
        // the last sound just ended or failed.
        let still = self.voices.iter().filter_map(|v| v.sound.upgrade()).any(|s| s.state().playing);
        if self.was_playing && !still {
            crate::quiet();
        }
        self.was_playing = still;
        if moved || self.since_notify >= (rate / 10).max(1) as usize {
            self.since_notify = 0;
            crate::notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Sound, State};
    use std::sync::Mutex;

    /// A sound of `frames` frames of one value, at `rate`.
    fn constant(value: f32, channels: usize, frames: usize, rate: u32) -> Sound {
        of(vec![value; frames * channels], channels, rate)
    }

    /// A sound of these samples.
    fn of(samples: Vec<f32>, channels: usize, rate: u32) -> Sound {
        let frames = samples.len() / channels;
        Sound(Arc::new(Shared {
            id: crate::NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            source: Source::Whole(Arc::new(Pcm { rate, channels, samples })),
            duration: Some(frames as f64 / rate as f64),
            state: Mutex::new(State::new()),
        }))
    }

    #[test]
    fn two_sounds_are_summed() {
        let m = Mutex::new(Mixer::new(48_000, 2));
        let a = constant(0.25, 1, 1_000, 48_000);
        let b = constant(0.5, 2, 1_000, 48_000);
        a.play_on(&m, 48_000).unwrap();
        b.play_on(&m, 48_000).unwrap();
        let mut out = vec![0.0; 2 * 100];
        m.lock().unwrap().mix(&mut out);
        assert!(out.iter().all(|s| (s - 0.75).abs() < 1e-6), "{:?}", &out[..4]);
        assert!((a.current_time() - 100.0 / 48_000.0).abs() < 1e-9);
    }

    #[test]
    fn a_paused_sound_is_silent_and_volume_scales() {
        let m = Mutex::new(Mixer::new(48_000, 1));
        let a = constant(0.5, 1, 1_000, 48_000);
        a.play_on(&m, 48_000).unwrap();
        a.set_volume(0.5);
        let mut out = vec![0.0; 10];
        m.lock().unwrap().mix(&mut out);
        assert!(out.iter().all(|s| (s - 0.25).abs() < 1e-6));
        a.pause();
        m.lock().unwrap().mix(&mut out);
        assert!(out.iter().all(|s| *s == 0.0));
    }

    #[test]
    fn a_sound_ends_then_starts_again() {
        let m = Mutex::new(Mixer::new(48_000, 1));
        let a = constant(1.0, 1, 10, 48_000);
        a.play_on(&m, 48_000).unwrap();
        let mut out = vec![0.0; 16];
        m.lock().unwrap().mix(&mut out);
        assert_eq!(out.iter().filter(|s| **s == 1.0).count(), 10, "the rest is silence");
        assert!(!a.playing() && a.ended() == 1);
        assert!((a.current_time() - 10.0 / 48_000.0).abs() < 1e-9, "left at its end");
        a.play_on(&m, 48_000).unwrap();
        assert_eq!(a.current_time(), 0.0, "and played from the start");
        m.lock().unwrap().mix(&mut out);
        assert_eq!(out.iter().filter(|s| **s == 1.0).count(), 10);
    }

    #[test]
    fn a_looping_sound_goes_round() {
        let m = Mutex::new(Mixer::new(48_000, 1));
        let a = constant(1.0, 1, 10, 48_000);
        a.set_looping(true);
        a.play_on(&m, 48_000).unwrap();
        let mut out = vec![0.0; 25];
        m.lock().unwrap().mix(&mut out);
        assert!(out.iter().all(|s| *s == 1.0));
        assert!(a.playing() && a.ended() == 0);
    }

    #[test]
    fn seek_and_stop() {
        let m = Mutex::new(Mixer::new(1_000, 1));
        let a = of((0..1_000).map(|i| i as f32).collect(), 1, 1_000);
        a.play_on(&m, 1_000).unwrap();
        a.seek(0.5);
        let mut out = vec![0.0; 2];
        m.lock().unwrap().mix(&mut out);
        assert_eq!(out, [500.0, 501.0]);
        a.stop();
        assert!(!a.playing() && a.current_time() == 0.0);
        a.play_on(&m, 1_000).unwrap();
        m.lock().unwrap().mix(&mut out);
        assert_eq!(out, [0.0, 1.0]);
    }

    #[test]
    fn a_new_output_picks_up_where_the_sound_was() {
        // The output opened again, after the app was away or the device
        // changed: the same sound, a new mixer, at another rate.
        let a = of((0..1_000).map(|i| i as f32).collect(), 1, 1_000);
        let first = Mutex::new(Mixer::new(1_000, 1));
        a.play_on(&first, 1_000).unwrap();
        let mut out = vec![0.0; 300];
        first.lock().unwrap().mix(&mut out);
        a.pause();
        assert!(!first.lock().unwrap().any_playing());
        assert_eq!(first.lock().unwrap().sounds().len(), 1);
        let second = Mutex::new(Mixer::new(1_000, 1));
        a.play_on(&second, 1_000).unwrap();
        let mut out = vec![0.0; 2];
        second.lock().unwrap().mix(&mut out);
        assert_eq!(out, [300.0, 301.0]);
        let twice = Mutex::new(Mixer::new(2_000, 1));
        a.play_on(&twice, 2_000).unwrap();
        twice.lock().unwrap().mix(&mut out);
        assert!((out[0] - 302.0).abs() < 1.0, "{out:?}");
    }
}
