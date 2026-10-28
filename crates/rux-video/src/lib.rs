//! Rux's video decoding: `docs/13-file-input-and-media-plan.md`, C.1.
//!
//! A [`Decoder`] reads a video's pictures in order, as RGBA [`Frame`]s with
//! the time each is shown at, and seeks. It is opened and used on one
//! thread: the player's, never the UI's, since a frame can take milliseconds.
//!
//! **Each platform decodes with its own decoder**, so no codec ships in Rux:
//! Media Foundation on Windows, `MediaCodec` on Android. Everywhere else
//! opening one is an error of kind `"unsupported"`, and the element shows
//! its poster. (The web plays a video through the browser, never here.)
//!
//! A video's sound is read apart, by a [`SoundTrack`] over the same bytes,
//! since it plays on the audio thread's terms and the pictures on the
//! player's.

use std::sync::Arc;

#[cfg(target_os = "android")]
mod android;
#[cfg(windows)]
mod windows;
#[cfg(any(target_os = "android", test))]
mod yuv;

/// Where Android writes a video's bytes for its extractor to read: the
/// app's own files. Set by the shell at start; see [`android`].
#[cfg(target_os = "android")]
pub use android::set_scratch_dir;

#[cfg(target_os = "android")]
use android as platform;
#[cfg(windows)]
use windows as platform;

/// Where a video's bytes come from.
#[derive(Clone, Debug)]
pub enum Input {
    /// The file itself, already read, with its extension or media type's
    /// subtype (`"mp4"`) as a hint to what it is.
    Bytes {
        bytes: Arc<[u8]>,
        hint: Option<String>,
    },
}

/// Why a video could not open or decode: `kind` is `"unsupported"`,
/// `"decode"` or `"io"`, as a sound's.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoError {
    pub kind: &'static str,
    pub message: String,
}

impl VideoError {
    #[allow(dead_code)]
    pub(crate) fn new(kind: &'static str, message: impl Into<String>) -> Self {
        VideoError {
            kind,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for VideoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for VideoError {}

/// What a video is, known once it is open.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    /// The picture's size as shown, in pixels: `videoWidth`, `videoHeight`.
    pub width: u32,
    pub height: u32,
    /// Its length in seconds, when the container says.
    pub duration: Option<f64>,
    /// It has a sound track as well.
    pub has_audio: bool,
}

/// One picture, decoded.
#[derive(Clone)]
pub struct Frame {
    /// When it is shown, in seconds from the start.
    pub time: f64,
    pub width: u32,
    pub height: u32,
    /// `width * height` pixels, top row first, as R, G, B, A bytes with alpha
    /// always 255.
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Frame({}x{} at {:.3}s)",
            self.width, self.height, self.time
        )
    }
}

/// A video being read.
pub struct Decoder {
    #[cfg(any(windows, target_os = "android"))]
    inner: platform::Decoder,
    info: Info,
}

impl Decoder {
    /// Open `input` and read what it is. No picture is decoded yet.
    pub fn open(input: Input) -> Result<Decoder, VideoError> {
        Self::open_on(input, true)
    }

    /// [`open`](Self::open), kept off the GPU's decoder when `gpu` is false:
    /// for the tests, which check both roads.
    #[doc(hidden)]
    pub fn open_on(input: Input, gpu: bool) -> Result<Decoder, VideoError> {
        #[cfg(any(windows, target_os = "android"))]
        {
            let inner = platform::Decoder::open(input, gpu)?;
            let info = inner.info();
            Ok(Decoder { inner, info })
        }
        #[cfg(not(any(windows, target_os = "android")))]
        {
            let _ = (input, gpu);
            Err(VideoError::new(
                "unsupported",
                "video does not play on this platform yet",
            ))
        }
    }

    pub fn info(&self) -> &Info {
        &self.info
    }

    /// The next picture; `None` at the end. After a [`seek`](Self::seek),
    /// the first is the one showing at the time asked for.
    pub fn next_frame(&mut self) -> Result<Option<Frame>, VideoError> {
        #[cfg(any(windows, target_os = "android"))]
        return self.inner.next_frame();
        #[cfg(not(any(windows, target_os = "android")))]
        Ok(None)
    }

    /// Go to `secs` from the start (clamped to the video).
    pub fn seek(&mut self, secs: f64) -> Result<(), VideoError> {
        #[cfg(any(windows, target_os = "android"))]
        return self.inner.seek(secs);
        #[cfg(not(any(windows, target_os = "android")))]
        {
            let _ = secs;
            Ok(())
        }
    }
}

/// A video's sound, read as interleaved `f32` samples at the file's own rate
/// by the same platform decoder as its pictures (C.3), so any sound the
/// platform plays (Opus in a WebM, AAC in an MP4) plays. Its times start at 0
/// as the pictures' do. The shell hands it to `rux-audio`'s mixer, whose
/// clock the pictures then follow.
pub struct SoundTrack {
    #[cfg(any(windows, target_os = "android"))]
    inner: platform::SoundTrack,
}

impl SoundTrack {
    /// Open the sound track of `input`; an error of kind `"unsupported"` when
    /// it has none, or none this machine decodes.
    pub fn open(input: Input) -> Result<SoundTrack, VideoError> {
        #[cfg(any(windows, target_os = "android"))]
        return Ok(SoundTrack { inner: platform::SoundTrack::open(input)? });
        #[cfg(not(any(windows, target_os = "android")))]
        {
            let _ = input;
            Err(VideoError::new("unsupported", "video does not play on this platform yet"))
        }
    }

    /// Frames per second.
    pub fn rate(&self) -> u32 {
        #[cfg(any(windows, target_os = "android"))]
        return self.inner.rate;
        #[cfg(not(any(windows, target_os = "android")))]
        0
    }

    /// Samples per frame.
    pub fn channels(&self) -> usize {
        #[cfg(any(windows, target_os = "android"))]
        return self.inner.channels;
        #[cfg(not(any(windows, target_os = "android")))]
        0
    }

    /// The next stretch of samples; `None` at the end.
    pub fn next_samples(&mut self) -> Result<Option<Vec<f32>>, VideoError> {
        #[cfg(any(windows, target_os = "android"))]
        return self.inner.next_samples();
        #[cfg(not(any(windows, target_os = "android")))]
        Ok(None)
    }

    /// Go to `secs` from the start: the next samples are the ones at it.
    pub fn seek(&mut self, secs: f64) -> Result<(), VideoError> {
        #[cfg(any(windows, target_os = "android"))]
        return self.inner.seek(secs);
        #[cfg(not(any(windows, target_os = "android")))]
        {
            let _ = secs;
            Ok(())
        }
    }
}
