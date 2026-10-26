//! Rux's sounds on the desktop and Android: `docs/13-file-input-and-media-plan.md`,
//! part B.
//!
//! A [`Sound`] is loaded from a file's bytes or its path and played through
//! one output stream shared by the whole app, mixing every sound that is
//! playing. The stream is opened by the first [`Sound::play`] and not
//! before, so an app that makes no sound never touches an audio device.
//!
//! **Short sounds are decoded whole, long ones stream.** A tap's click is
//! decoded once into memory and plays at once; a song is read a little ahead
//! of where it plays, on a thread of its own, so it is never held decoded.
//!
//! Formats are symphonia's: WAV, MP3, AAC in MP4, FLAC and Ogg Vorbis. **Not
//! Opus**, which symphonia does not decode: loading one is an error.
//! The web plays through the browser instead, and does not use this crate.
//!
//! **AAC in MP4 keeps its encoder's priming**: about 23 ms of near-silence
//! before the sound and a few at its end. An MP4 says to skip them in an
//! edit list, which symphonia's MP4 reader does not apply, so its packets
//! carry no trim for [`decode`] to make. MP3's, which its reader does mark,
//! are trimmed.

mod decode;
mod mixer;
#[cfg(not(target_arch = "wasm32"))]
mod output;
mod resample;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

pub use mixer::Mixer;

/// Where a sound's bytes come from.
#[derive(Clone, Debug)]
pub enum Input {
    /// The file itself, already read, with its extension or media type's
    /// subtype (`"mp3"`) as a hint to what it is.
    Bytes { bytes: Arc<[u8]>, hint: Option<String> },
    /// A file on disk, read as it plays.
    Path(PathBuf),
}

/// Why a sound could not load or play: `kind` is `"unsupported"`, `"decode"`,
/// `"io"` or `"device"`.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioError {
    pub kind: &'static str,
    pub message: String,
}

impl AudioError {
    pub(crate) fn new(kind: &'static str, message: impl Into<String>) -> Self {
        AudioError { kind, message: message.into() }
    }
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AudioError {}

/// A sound loaded and ready to play. Clones are the same sound.
#[derive(Clone)]
pub struct Sound(pub(crate) Arc<Shared>);

/// Decoded samples, interleaved.
pub(crate) struct Pcm {
    pub rate: u32,
    pub channels: usize,
    pub samples: Vec<f32>,
}

pub(crate) enum Source {
    Whole(Arc<Pcm>),
    Stream(Input),
}

/// What a sound and the mixer playing it share.
pub(crate) struct Shared {
    pub id: u64,
    pub source: Source,
    pub duration: Option<f64>,
    pub state: Mutex<State>,
}

impl Shared {
    pub fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(Debug)]
pub(crate) struct State {
    pub playing: bool,
    pub volume: f32,
    pub looping: bool,
    /// Where it is, in seconds.
    pub current: f64,
    /// A seek the mixer has not made yet.
    pub seek: Option<f64>,
    /// It played to its end, and the next `play` starts it again.
    pub at_end: bool,
    /// How many times it has played to its end.
    pub ended: u64,
    /// Why it stopped, when reading it failed part way.
    pub error: Option<AudioError>,
}

impl State {
    /// A sound not yet played: at its start, at full volume.
    pub fn new() -> Self {
        State {
            playing: false,
            volume: 1.0,
            looping: false,
            current: 0.0,
            seek: None,
            at_end: false,
            ended: 0,
            error: None,
        }
    }
}

/// Sounds shorter than this are decoded whole.
const WHOLE_UNDER_SECS: f64 = 10.0;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

type Notify = Arc<dyn Fn() + Send + Sync>;
static NOTIFY: Mutex<Option<Notify>> = Mutex::new(None);

/// Call `f` whenever a sound's state moves on its own: about ten times a
/// second while any plays, and when one ends or fails. From the audio
/// thread, so `f` only wakes whoever reads the sounds.
pub fn set_notify(f: impl Fn() + Send + Sync + 'static) {
    *NOTIFY.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(f));
}

static QUIET: Mutex<Option<Notify>> = Mutex::new(None);

/// Call `f` when the last sound playing stops by itself: it ended, or
/// reading it failed. From the audio thread, so `f` must not wait on it:
/// hand anything slow to another thread.
pub fn set_quiet(f: impl Fn() + Send + Sync + 'static) {
    *QUIET.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(f));
}

pub(crate) fn quiet() {
    let f = QUIET.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(f) = f {
        f();
    }
}

pub(crate) fn notify() {
    let f = NOTIFY.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(f) = f {
        f();
    }
}

/// Pause every sound and let the output device go, as when the app is put
/// away or another app takes the sound. Nothing starts again by itself; the
/// next [`Sound::play`] opens the output again.
#[cfg(not(target_arch = "wasm32"))]
pub fn pause_all() {
    output::pause_all();
}

/// Whether any sound is playing.
#[cfg(not(target_arch = "wasm32"))]
pub fn any_playing() -> bool {
    output::any_playing()
}

/// What the person can hear: `0` nothing, `1` only short sounds (under ten
/// seconds, decoded whole), `2` a long one. A sound playing at volume 0
/// (an `<audio muted autoplay>`) is not heard, so it holds no audio focus.
#[cfg(not(target_arch = "wasm32"))]
pub fn audible() -> u8 {
    output::audible()
}

impl Sound {
    /// Read `input` far enough to know it is a sound Rux can play: all of it
    /// when it is short, its header when it is long.
    pub fn load(input: Input) -> Result<Sound, AudioError> {
        Self::load_with(input, WHOLE_UNDER_SECS)
    }

    pub(crate) fn load_with(input: Input, whole_under: f64) -> Result<Sound, AudioError> {
        let mut d = decode::Decoding::open(&input)?;
        let short = d.duration.is_some_and(|s| s < whole_under);
        let (source, duration) = if short {
            let samples = decode::decode_all(&mut d)?;
            let channels = d.channels.max(1);
            let frames = samples.len() / channels;
            let duration = (d.rate > 0).then(|| frames as f64 / d.rate as f64).or(d.duration);
            (Source::Whole(Arc::new(Pcm { rate: d.rate, channels, samples })), duration)
        } else {
            // Read once more to be sure it decodes, so a broken file fails at
            // `load` and not partway through playing.
            d.next()?;
            (Source::Stream(input), d.duration)
        };
        Ok(Sound(Arc::new(Shared {
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            source,
            duration,
            state: Mutex::new(State::new()),
        })))
    }

    /// Whether it is short enough to have been decoded whole (under ten
    /// seconds): a tap's click, not a song.
    pub fn is_short(&self) -> bool {
        matches!(self.0.source, Source::Whole(_))
    }

    /// Its length in seconds, when the file says.
    pub fn duration(&self) -> Option<f64> {
        self.0.duration
    }

    /// Where it is, in seconds.
    pub fn current_time(&self) -> f64 {
        self.0.state().current
    }

    pub fn playing(&self) -> bool {
        self.0.state().playing
    }

    pub fn volume(&self) -> f32 {
        self.0.state().volume
    }

    /// From 0 (silent) to 1 (as recorded).
    pub fn set_volume(&self, v: f32) {
        self.0.state().volume = if v.is_finite() { v.clamp(0.0, 1.0) } else { 1.0 };
    }

    pub fn looping(&self) -> bool {
        self.0.state().looping
    }

    pub fn set_looping(&self, on: bool) {
        self.0.state().looping = on;
    }

    /// How many times it has played to its end.
    pub fn ended(&self) -> u64 {
        self.0.state().ended
    }

    /// Why it stopped, if reading it failed while it played.
    pub fn error(&self) -> Option<AudioError> {
        self.0.state().error.clone()
    }

    /// Play from where it is, or from the start after it ended. Opens the
    /// app's output the first time any sound plays.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn play(&self) -> Result<(), AudioError> {
        let (mixer, rate) = output::mixer()?;
        self.play_on(&mixer, rate)
    }

    /// [`Sound::play`] on a given mixer, as a test plays one without a device.
    pub fn play_on(&self, mixer: &Mutex<Mixer>, rate: u32) -> Result<(), AudioError> {
        let present = mixer.lock().unwrap_or_else(|e| e.into_inner()).has(self.0.id);
        if !present {
            // Made before the lock: resampling a clip takes a moment, and the
            // audio thread waits on that lock.
            let voice = mixer::Voice::new(&self.0, rate)?;
            mixer.lock().unwrap_or_else(|e| e.into_inner()).add(voice);
        }
        let mut st = self.0.state();
        if st.at_end {
            st.at_end = false;
            st.current = 0.0;
        }
        st.error = None;
        st.playing = true;
        Ok(())
    }

    pub fn pause(&self) {
        self.0.state().playing = false;
    }

    /// Pause, and go back to the start.
    pub fn stop(&self) {
        let mut st = self.0.state();
        st.playing = false;
        st.at_end = false;
        st.current = 0.0;
        st.seek = Some(0.0);
    }

    /// Go to `secs`, clamped to the sound.
    pub fn seek(&self, secs: f64) {
        let end = self.0.duration.unwrap_or(f64::MAX);
        let secs = if secs.is_finite() { secs.clamp(0.0, end) } else { 0.0 };
        let mut st = self.0.state();
        st.at_end = false;
        st.current = secs;
        st.seek = Some(secs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wav() -> Input {
        Input::Path(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tone.wav"))
    }

    /// Mix `sound` until it stops or `limit` passes, the reading thread
    /// filling in behind: every left sample played.
    fn play_out(sound: &Sound, m: &Mutex<Mixer>, limit: Duration) -> Vec<f32> {
        let start = Instant::now();
        let mut all = Vec::new();
        let mut out = vec![0.0; 2 * 512];
        while sound.playing() && start.elapsed() < limit {
            m.lock().unwrap().mix(&mut out);
            all.extend(out.chunks(2).map(|f| f[0]));
            std::thread::sleep(Duration::from_millis(1));
        }
        all
    }

    #[test]
    fn a_long_sound_streams_the_same_samples() {
        let whole = Sound::load_with(wav(), 10.0).unwrap();
        let streamed = Sound::load_with(wav(), 0.0).unwrap();
        assert!(matches!(whole.0.source, Source::Whole(_)) && matches!(streamed.0.source, Source::Stream(_)));
        let m = Mutex::new(Mixer::new(44_100, 2));
        whole.play_on(&m, 44_100).unwrap();
        let a = play_out(&whole, &m, Duration::from_secs(5));
        streamed.play_on(&m, 44_100).unwrap();
        let b: Vec<f32> = play_out(&streamed, &m, Duration::from_secs(5)).into_iter().filter(|s| *s != 0.0).collect();
        let a: Vec<f32> = a.into_iter().filter(|s| *s != 0.0).collect();
        assert_eq!(a.len(), b.len(), "the same frames, a pause for reading aside");
        assert!(a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-6));
        assert_eq!(streamed.ended(), 1);
    }

    #[test]
    fn a_stream_seeks_and_loops() {
        let s = Sound::load_with(wav(), 0.0).unwrap();
        let m = Mutex::new(Mixer::new(44_100, 2));
        s.seek(0.2);
        s.play_on(&m, 44_100).unwrap();
        let rest = play_out(&s, &m, Duration::from_secs(5));
        let played = rest.iter().filter(|x| **x != 0.0).count() as f64 / 44_100.0;
        assert!((played - 0.05).abs() < 0.01, "the last twentieth of a second, {played}");
        s.set_looping(true);
        s.play_on(&m, 44_100).unwrap();
        let round = play_out(&s, &m, Duration::from_millis(800));
        assert!(s.playing(), "still going round");
        assert!(round.iter().filter(|x| **x != 0.0).count() > 44_100 / 2, "more than once through");
    }
}
