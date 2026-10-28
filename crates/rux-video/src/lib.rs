//! Rux's video decoding: `docs/13-file-input-and-media-plan.md`, C.1.
//!
//! A [`Decoder`] reads a video's pictures in order, as RGBA [`Frame`]s with
//! the time each is shown at, and seeks. It is opened and used on one
//! thread: the player's, never the UI's, since a frame can take milliseconds.
//!
//! **Each platform decodes with its own decoder**, so no codec ships in Rux:
//! Media Foundation on Windows. Everywhere else opening one is an error of
//! kind `"unsupported"`, and the element shows its poster.
//!
//! Only the pictures come from here. A video's sound is C.3's.

use std::sync::Arc;

#[cfg(windows)]
mod windows;

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
    #[cfg(windows)]
    inner: windows::Decoder,
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
        #[cfg(windows)]
        {
            let inner = windows::Decoder::open(input, gpu)?;
            let info = inner.info();
            Ok(Decoder { inner, info })
        }
        #[cfg(not(windows))]
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
        #[cfg(windows)]
        return self.inner.next_frame();
        #[cfg(not(windows))]
        Ok(None)
    }

    /// Go to `secs` from the start (clamped to the video).
    pub fn seek(&mut self, secs: f64) -> Result<(), VideoError> {
        #[cfg(windows)]
        return self.inner.seek(secs);
        #[cfg(not(windows))]
        {
            let _ = secs;
            Ok(())
        }
    }
}
