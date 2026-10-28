//! Android: the NDK's `MediaExtractor` reads the container and `MediaCodec`
//! decodes, in hardware where the phone has it, handing pictures back as
//! YUV in a buffer, which is turned into RGBA here, and sound as 16-bit
//! samples. Both from API 21, under Rux's 26.
//!
//! **The bytes reach the extractor as a file.** Its in-memory source,
//! `AMediaDataSource`, is API 28, and linking it would stop the app loading
//! on Android 8; so they are written once into the app's own files, opened,
//! and the name unlinked at once, the descriptor alone keeping them
//! ([`set_scratch_dir`]).
//!
//! **Pictures are read in the two layouts every decoder offers in a buffer**:
//! planar I420 (`COLOR_FormatYUV420Planar`, 19) and semi-planar NV12
//! (`COLOR_FormatYUV420SemiPlanar`, 21), the flexible format resolving to one
//! of them. A vendor's own tiled layout is an error of kind `"unsupported"`.
//! The colour matrix is the stream's when it says (`color-standard`), else
//! BT.601 under 720 rows and BT.709 from there, as browsers guess.

use std::ffi::CStr;
use std::os::raw::c_char;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use ndk_sys as sys;

use crate::yuv::{Layout, COLOR_I420};
use crate::{Frame, Info, Input, VideoError};

/// Where the bytes of a video being played are written: the app's own
/// files, set by the shell at start.
static SCRATCH: OnceLock<PathBuf> = OnceLock::new();

/// Name the folder the extractor's files are written in.
pub fn set_scratch_dir(dir: PathBuf) {
    let _ = SCRATCH.set(dir);
}

/// As Windows' [`crate::windows`] has it: a first picture stamped later than
/// zero by less than this is taken as the start.
const START_SHIFT: f64 = 1.0;

fn ok(status: sys::media_status_t) -> bool {
    status == sys::media_status_t::AMEDIA_OK
}

/// A format's integer, by its key's name.
fn int(format: *mut sys::AMediaFormat, key: &CStr) -> Option<i32> {
    let mut v = 0;
    // SAFETY: a live format and a NUL-terminated key.
    unsafe { sys::AMediaFormat_getInt32(format, key.as_ptr(), &mut v) }.then_some(v)
}

fn long(format: *mut sys::AMediaFormat, key: &CStr) -> Option<i64> {
    let mut v = 0;
    // SAFETY: as in `int`.
    unsafe { sys::AMediaFormat_getInt64(format, key.as_ptr(), &mut v) }.then_some(v)
}

fn mime(format: *mut sys::AMediaFormat) -> Option<String> {
    let mut p: *const c_char = std::ptr::null();
    // SAFETY: the key is the NDK's own; the string is the format's, copied
    // before the format goes.
    unsafe {
        if !sys::AMediaFormat_getString(format, sys::AMEDIAFORMAT_KEY_MIME, &mut p) || p.is_null() {
            return None;
        }
        Some(CStr::from_ptr(p).to_string_lossy().into_owned())
    }
}

/// An extractor over a video's bytes, keeping the unlinked file it reads.
struct Extractor {
    ptr: *mut sys::AMediaExtractor,
    _file: std::fs::File,
}

impl Drop for Extractor {
    fn drop(&mut self) {
        // SAFETY: made by `AMediaExtractor_new`, deleted once.
        unsafe { sys::AMediaExtractor_delete(self.ptr) };
    }
}

impl Extractor {
    fn open(bytes: &[u8]) -> Result<Extractor, VideoError> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = SCRATCH.get().cloned().unwrap_or_else(std::env::temp_dir);
        let path = dir.join(format!("rux-video-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let io = |e: std::io::Error| VideoError::new("io", format!("cannot hold the video: {e}"));
        std::fs::write(&path, bytes).map_err(io)?;
        let file = std::fs::File::open(&path).map_err(io);
        let _ = std::fs::remove_file(&path);
        let file = file?;
        use std::os::fd::AsRawFd;
        // SAFETY: a new extractor over a descriptor kept open as long as it.
        let ptr = unsafe { sys::AMediaExtractor_new() };
        if ptr.is_null() {
            return Err(VideoError::new("unsupported", "this phone has no media extractor"));
        }
        let ex = Extractor { ptr, _file: file };
        let fd = ex._file.as_raw_fd();
        if !ok(unsafe { sys::AMediaExtractor_setDataSourceFd(ex.ptr, fd, 0, bytes.len() as i64) }) {
            return Err(VideoError::new("unsupported", "not a video Rux can read"));
        }
        Ok(ex)
    }

    /// The first track whose type starts `kind` (`"video/"`, `"audio/"`):
    /// its index and format, which the caller deletes.
    fn track(&self, kind: &str) -> Option<(usize, *mut sys::AMediaFormat)> {
        // SAFETY: a live extractor; each format not returned is deleted.
        unsafe {
            for i in 0..sys::AMediaExtractor_getTrackCount(self.ptr) {
                let f = sys::AMediaExtractor_getTrackFormat(self.ptr, i);
                if f.is_null() {
                    continue;
                }
                if mime(f).is_some_and(|m| m.starts_with(kind)) {
                    return Some((i, f));
                }
                sys::AMediaFormat_delete(f);
            }
        }
        None
    }
}

/// One track decoded: its extractor and its codec, fed and drained in turn.
struct Track {
    ex: Extractor,
    codec: *mut sys::AMediaCodec,
    /// The extractor has no more to give; the codec has given its last.
    fed: bool,
    drained: bool,
}

/// What the codec handed back.
enum Out {
    /// A buffer's bytes, at its time in seconds.
    Data(f64, Vec<u8>),
    /// Its output format changed: read it again.
    Format,
    End,
}

impl Drop for Track {
    fn drop(&mut self) {
        // SAFETY: made and started by `Track::open`, deleted once.
        unsafe {
            sys::AMediaCodec_stop(self.codec);
            sys::AMediaCodec_delete(self.codec);
        }
    }
}

impl Track {
    /// Open the first `kind` track of `bytes` and its decoder: the track and
    /// its format, which the caller deletes.
    fn open(bytes: &[u8], kind: &str, none: &'static str) -> Result<(Track, *mut sys::AMediaFormat), VideoError> {
        let ex = Extractor::open(bytes)?;
        let (index, format) = ex.track(kind).ok_or_else(|| VideoError::new("unsupported", none))?;
        let fail = |message: String| {
            // SAFETY: the format from `track`, not handed on.
            unsafe { sys::AMediaFormat_delete(format) };
            VideoError::new("unsupported", message)
        };
        let ty = mime(format).unwrap_or_default();
        let cty = std::ffi::CString::new(ty.clone()).unwrap_or_default();
        // SAFETY: a live extractor and format; the codec is deleted by `Drop`
        // once it is in a `Track`, and here when it fails before.
        unsafe {
            if !ok(sys::AMediaExtractor_selectTrack(ex.ptr, index)) {
                return Err(fail(format!("cannot read its {ty} track")));
            }
            let codec = sys::AMediaCodec_createDecoderByType(cty.as_ptr());
            if codec.is_null() {
                return Err(fail(format!("this phone has no decoder for {ty}")));
            }
            if !ok(sys::AMediaCodec_configure(codec, format, std::ptr::null_mut(), std::ptr::null_mut(), 0))
                || !ok(sys::AMediaCodec_start(codec))
            {
                sys::AMediaCodec_delete(codec);
                return Err(fail(format!("this phone's {ty} decoder would not start")));
            }
            Ok((Track { ex, codec, fed: false, drained: false }, format))
        }
    }

    /// Feed the codec what it will take, then take what it has: the next
    /// buffer out, a change of format, or the end.
    fn next(&mut self) -> Result<Out, VideoError> {
        let decode = |what: &str| VideoError::new("decode", format!("it stopped decoding: {what}"));
        loop {
            // SAFETY: a started codec and live extractor; each buffer index is
            // the codec's own and handed back once, its bytes copied first.
            unsafe {
                while !self.fed {
                    let i = sys::AMediaCodec_dequeueInputBuffer(self.codec, 0);
                    if i < 0 {
                        break;
                    }
                    let mut cap = 0usize;
                    let buf = sys::AMediaCodec_getInputBuffer(self.codec, i as usize, &mut cap);
                    let n = if buf.is_null() { -1 } else { sys::AMediaExtractor_readSampleData(self.ex.ptr, buf, cap) };
                    if n < 0 {
                        self.fed = true;
                        let eos = sys::AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM;
                        sys::AMediaCodec_queueInputBuffer(self.codec, i as usize, 0, 0, 0, eos);
                    } else {
                        let t = sys::AMediaExtractor_getSampleTime(self.ex.ptr).max(0) as u64;
                        sys::AMediaCodec_queueInputBuffer(self.codec, i as usize, 0, n as usize, t, 0);
                        sys::AMediaExtractor_advance(self.ex.ptr);
                    }
                }
                if self.drained {
                    return Ok(Out::End);
                }
                let mut info = sys::AMediaCodecBufferInfo { offset: 0, size: 0, presentationTimeUs: 0, flags: 0 };
                let o = sys::AMediaCodec_dequeueOutputBuffer(self.codec, &mut info, 10_000);
                if o >= 0 {
                    let end = info.flags & sys::AMEDIACODEC_BUFFER_FLAG_END_OF_STREAM != 0;
                    let mut cap = 0usize;
                    let buf = sys::AMediaCodec_getOutputBuffer(self.codec, o as usize, &mut cap);
                    let data = if buf.is_null() || info.size <= 0 {
                        Vec::new()
                    } else {
                        let at = info.offset.max(0) as usize;
                        let n = (info.size as usize).min(cap.saturating_sub(at));
                        std::slice::from_raw_parts(buf.add(at), n).to_vec()
                    };
                    sys::AMediaCodec_releaseOutputBuffer(self.codec, o as usize, false);
                    self.drained = end;
                    if data.is_empty() {
                        continue;
                    }
                    return Ok(Out::Data(info.presentationTimeUs as f64 / 1e6, data));
                }
                match o as i32 {
                    sys::AMEDIACODEC_INFO_OUTPUT_FORMAT_CHANGED => return Ok(Out::Format),
                    sys::AMEDIACODEC_INFO_TRY_AGAIN_LATER | sys::AMEDIACODEC_INFO_OUTPUT_BUFFERS_CHANGED => {}
                    other => return Err(decode(&format!("status {other}"))),
                }
            }
        }
    }

    fn format(&self) -> *mut sys::AMediaFormat {
        // SAFETY: a started codec; the caller deletes the copy.
        unsafe { sys::AMediaCodec_getOutputFormat(self.codec) }
    }

    /// Back to the key frame at or before `secs`, everything in flight let go.
    fn seek(&mut self, secs: f64) -> Result<(), VideoError> {
        // SAFETY: a live extractor and started codec.
        unsafe {
            let mode = sys::SeekMode::AMEDIAEXTRACTOR_SEEK_PREVIOUS_SYNC;
            if !ok(sys::AMediaExtractor_seekTo(self.ex.ptr, (secs.max(0.0) * 1e6) as i64, mode)) {
                return Err(VideoError::new("decode", "cannot seek in it"));
            }
            sys::AMediaCodec_flush(self.codec);
        }
        self.fed = false;
        self.drained = false;
        Ok(())
    }
}

/// How the codec lays out its pictures, from its output format.
fn layout(format: *mut sys::AMediaFormat, info: &Info) -> Layout {
    let get = |k: &CStr| int(format, k).filter(|v| *v > 0).map(|v| v as usize);
    let width = get(c"width").unwrap_or(info.width as usize);
    let height = get(c"height").unwrap_or(info.height as usize);
    let (left, top) = (int(format, c"crop-left").unwrap_or(0).max(0) as usize, int(format, c"crop-top").unwrap_or(0).max(0) as usize);
    let right = int(format, c"crop-right").map_or(width.saturating_sub(1), |v| v.max(0) as usize);
    let bottom = int(format, c"crop-bottom").map_or(height.saturating_sub(1), |v| v.max(0) as usize);
    let shown = (right + 1).saturating_sub(left).min(width);
    let rows_shown = (bottom + 1).saturating_sub(top).min(height);
    // 1 is BT.709; 2 and 4 are BT.601; nothing said is guessed by size.
    let hd = match int(format, c"color-standard") {
        Some(1) => true,
        Some(2 | 4) => false,
        _ => rows_shown >= 720,
    };
    Layout {
        color: int(format, c"color-format").unwrap_or(COLOR_I420),
        stride: get(c"stride").unwrap_or(width).max(width),
        rows: get(c"slice-height").unwrap_or(height).max(height),
        left,
        top,
        width: shown,
        height: rows_shown,
        hd,
        full: int(format, c"color-range") == Some(1),
    }
}

pub(crate) struct Decoder {
    track: Track,
    info: Info,
    layout: Option<Layout>,
    /// The first picture's time, taken off every time ([`START_SHIFT`]).
    offset: f64,
    /// A seek's target, until the picture showing at it is read.
    seeking: Option<f64>,
    /// A picture read ahead: the first, read at open, or the one after a
    /// seek's that told the seek where to stop.
    held: Option<Frame>,
}

impl Decoder {
    pub fn open(input: Input, _gpu: bool) -> Result<Decoder, VideoError> {
        let Input::Bytes { bytes, .. } = input;
        let (track, format) = Track::open(&bytes, "video/", "it has no picture in it")?;
        let has_audio = track.ex.track("audio/").map(|(_, f)| unsafe { sys::AMediaFormat_delete(f) }).is_some();
        let info = Info {
            width: int(format, c"width").unwrap_or(0).max(0) as u32,
            height: int(format, c"height").unwrap_or(0).max(0) as u32,
            duration: long(format, c"durationUs").filter(|d| *d > 0).map(|d| d as f64 / 1e6),
            has_audio,
        };
        // SAFETY: the format `Track::open` handed over.
        unsafe { sys::AMediaFormat_delete(format) };
        let mut d = Decoder { track, info, layout: None, offset: 0.0, seeking: None, held: None };
        if let Some(first) = d.read()? {
            if first.time > 0.0 && first.time < START_SHIFT {
                d.offset = first.time;
            }
            d.held = Some(Frame { time: first.time - d.offset, ..first });
        }
        // The picture's own size, crop applied, once a picture says it.
        if let Some(l) = d.layout {
            d.info.width = l.width as u32;
            d.info.height = l.height as u32;
        }
        Ok(d)
    }

    pub fn info(&self) -> Info {
        self.info.clone()
    }

    /// The next picture as the codec stamps it.
    fn read(&mut self) -> Result<Option<Frame>, VideoError> {
        loop {
            match self.track.next()? {
                Out::End => return Ok(None),
                Out::Format => {
                    let f = self.track.format();
                    self.layout = Some(layout(f, &self.info));
                    // SAFETY: the copy `format` handed over.
                    unsafe { sys::AMediaFormat_delete(f) };
                }
                Out::Data(time, data) => {
                    let layout = match self.layout {
                        Some(l) => l,
                        None => {
                            let f = self.track.format();
                            let l = layout(f, &self.info);
                            // SAFETY: as above.
                            unsafe { sys::AMediaFormat_delete(f) };
                            self.layout = Some(l);
                            l
                        }
                    };
                    let rgba = layout.rgba(&data)?;
                    return Ok(Some(Frame { time, width: layout.width as u32, height: layout.height as u32, rgba }));
                }
            }
        }
    }

    pub fn next_frame(&mut self) -> Result<Option<Frame>, VideoError> {
        if let Some(f) = self.held.take() {
            return Ok(Some(f));
        }
        let shift = |f: Frame, offset: f64| Frame { time: (f.time - offset).max(0.0), ..f };
        let Some(target) = self.seeking.take() else {
            return Ok(self.read()?.map(|f| shift(f, self.offset)));
        };
        // A seek lands on the key frame before its target: read on, keeping
        // the latest picture at or before it, until the one after.
        let mut showing: Option<Frame> = None;
        while let Some(f) = self.read()?.map(|f| shift(f, self.offset)) {
            if f.time <= target + 1e-4 || showing.is_none() {
                showing = Some(f);
                continue;
            }
            self.held = Some(f);
            break;
        }
        Ok(showing)
    }

    pub fn seek(&mut self, secs: f64) -> Result<(), VideoError> {
        let secs = match self.info.duration {
            Some(d) => secs.clamp(0.0, d),
            None => secs.max(0.0),
        };
        self.track.seek(secs + self.offset)?;
        self.held = None;
        self.seeking = Some(secs);
        Ok(())
    }
}

/// A video's sound, decoded by `MediaCodec` to 16-bit samples and given out
/// as `f32`.
pub(crate) struct SoundTrack {
    track: Track,
    pub rate: u32,
    pub channels: usize,
    /// 16-bit samples, or floats when the codec says `pcm-encoding` 4.
    float: bool,
    seeking: Option<f64>,
}

impl SoundTrack {
    pub fn open(input: Input) -> Result<SoundTrack, VideoError> {
        let Input::Bytes { bytes, .. } = input;
        let (track, format) = Track::open(&bytes, "audio/", "it has no sound in it")?;
        let rate = int(format, c"sample-rate").unwrap_or(0).max(0) as u32;
        let channels = int(format, c"channel-count").unwrap_or(0).max(0) as usize;
        // SAFETY: the format `Track::open` handed over.
        unsafe { sys::AMediaFormat_delete(format) };
        if rate == 0 || channels == 0 {
            return Err(VideoError::new("decode", "its sound has no rate or channels"));
        }
        Ok(SoundTrack { track, rate, channels, float: false, seeking: None })
    }

    pub fn next_samples(&mut self) -> Result<Option<Vec<f32>>, VideoError> {
        loop {
            match self.track.next()? {
                Out::End => return Ok(None),
                Out::Format => {
                    let f = self.track.format();
                    if let Some(r) = int(f, c"sample-rate").filter(|r| *r > 0) {
                        self.rate = r as u32;
                    }
                    if let Some(c) = int(f, c"channel-count").filter(|c| *c > 0) {
                        self.channels = c as usize;
                    }
                    self.float = int(f, c"pcm-encoding") == Some(4);
                    // SAFETY: the copy `format` handed over.
                    unsafe { sys::AMediaFormat_delete(f) };
                }
                Out::Data(time, data) => {
                    let mut samples: Vec<f32> = if self.float {
                        data.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()
                    } else {
                        data.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0).collect()
                    };
                    samples.truncate(samples.len() / self.channels * self.channels);
                    if let Some(target) = self.seeking {
                        let frames = samples.len() / self.channels;
                        let skip = ((target - time) * self.rate as f64).round().max(0.0) as usize;
                        if skip >= frames {
                            continue;
                        }
                        samples.drain(..skip * self.channels);
                        self.seeking = None;
                    }
                    if !samples.is_empty() {
                        return Ok(Some(samples));
                    }
                }
            }
        }
    }

    pub fn seek(&mut self, secs: f64) -> Result<(), VideoError> {
        self.track.seek(secs)?;
        self.seeking = Some(secs.max(0.0));
        Ok(())
    }
}
