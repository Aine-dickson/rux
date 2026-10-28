//! Windows: Media Foundation's source reader, asked for RGB32 so its own
//! video processor converts the colour and deinterlaces. The picture shown is
//! cut from the one decoded here, by the display aperture, since a stream is
//! coded in whole macroblocks (120 rows come out as 128).
//!
//! **On the GPU where there is one** (a D3D11 device handed to Media
//! Foundation, and its advanced video processor), and the plain software
//! processor where there is not. The frame comes back to the CPU either
//! way; the shell's texture road takes it from there (C.0).
//!
//! **A file that does not say its colour matrix** is guessed at per decoder:
//! H.264 as BT.601, VP9 as BT.709, so the same untagged picture's pure green
//! came out 255 from one and 215 from the other. Tagged files agree.
//!
//! The bytes reach it as an in-memory stream (`SHCreateMemStream`), with the
//! hint as its content type: a byte stream has no file name for Media
//! Foundation to guess the container from, and WebM is not sniffed.

use std::sync::Once;

use windows::core::{Interface, HSTRING};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::UI::Shell::SHCreateMemStream;

use crate::{Frame, Info, Input, VideoError};

const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
const AUDIO: u32 = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
/// Media Foundation counts time in 100 ns ticks.
const TICKS: f64 = 10_000_000.0;

fn err(kind: &'static str, what: &'static str) -> impl Fn(windows::core::Error) -> VideoError {
    move |e| VideoError::new(kind, format!("{what}: {}", e.message()))
}

/// COM on this thread, and Media Foundation once for the process. Neither is
/// ever shut down: a player's thread ends with the video, and the process
/// with the app.
fn start() -> Result<(), VideoError> {
    static STARTED: Once = Once::new();
    static mut STARTUP: Result<(), String> = Ok(());
    // A thread already in a single-threaded apartment says so and is still
    // fine to use: the reader is free-threaded.
    let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    STARTED.call_once(|| unsafe {
        STARTUP = MFStartup(MF_VERSION, MFSTARTUP_FULL).map_err(|e| e.message());
    });
    #[allow(static_mut_refs)]
    match unsafe { &STARTUP } {
        Ok(()) => Ok(()),
        Err(m) => Err(VideoError::new(
            "unsupported",
            format!("Media Foundation did not start: {m}"),
        )),
    }
}

/// A media type for `hint`, as a byte stream's content type.
fn content_type(hint: Option<&str>) -> Option<&'static str> {
    Some(match hint?.to_ascii_lowercase().as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        "wmv" => "video/x-ms-wmv",
        "3gp" => "video/3gpp",
        _ => return None,
    })
}

pub(crate) struct Decoder {
    reader: IMFSourceReader,
    width: u32,
    height: u32,
    /// Where the shown picture starts in the decoded one, and its row length
    /// in bytes when the buffer does not say.
    left: u32,
    top: u32,
    stride: i32,
    duration: Option<f64>,
    has_audio: bool,
    /// A seek's target, until a frame showing at it is read.
    seeking: Option<f64>,
}

/// A D3D11 device for Media Foundation to decode on, so the GPU's own
/// decoder and video processor do the work: 1080p60 H.264 took 16.3 ms a
/// frame in software on the dev machine, the whole of a 60 Hz frame. `None`
/// where there is no such device (a remote session, no driver), and the
/// software path is used.
///
/// One for the process, made by the first video opened: making it is most of
/// the 200 ms an open took, and the manager is built to be shared, locking
/// the device for each user in turn.
fn hardware() -> Option<IMFDXGIDeviceManager> {
    struct Shared(Option<IMFDXGIDeviceManager>);
    // SAFETY: the manager is free-threaded, and the device under it is
    // multithread-protected below.
    unsafe impl Send for Shared {}
    unsafe impl Sync for Shared {}
    static DEVICE: std::sync::OnceLock<Shared> = std::sync::OnceLock::new();
    DEVICE.get_or_init(|| Shared(new_device())).0.clone()
}

fn new_device() -> Option<IMFDXGIDeviceManager> {
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::*;
    unsafe {
        let mut device = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            Default::default(),
            D3D11_CREATE_DEVICE_VIDEO_SUPPORT | D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )
        .ok()?;
        let device = device?;
        // Media Foundation reaches the device from threads of its own.
        let _ = device
            .cast::<ID3D11Multithread>()
            .ok()?
            .SetMultithreadProtected(true);
        let (mut token, mut manager) = (0, None);
        MFCreateDXGIDeviceManager(&mut token, &mut manager).ok()?;
        let manager = manager?;
        manager.ResetDevice(&device, token).ok()?;
        Some(manager)
    }
}

/// A source reader over `bytes`, its video stream alone selected and set to
/// hand out RGB32: on `gpu` when given, in software otherwise.
fn reader(
    bytes: &[u8],
    hint: Option<&str>,
    gpu: Option<&IMFDXGIDeviceManager>,
) -> Result<IMFSourceReader, VideoError> {
    unsafe {
        let stream = SHCreateMemStream(Some(bytes))
            .ok_or_else(|| VideoError::new("io", "no memory for the video"))?;
        let bytestream =
            MFCreateMFByteStreamOnStream(&stream).map_err(err("io", "cannot read the video"))?;
        if let (Some(ty), Ok(attrs)) = (content_type(hint), bytestream.cast::<IMFAttributes>()) {
            let _ = attrs.SetString(&MF_BYTESTREAM_CONTENT_TYPE, &HSTRING::from(ty));
        }
        let mut attrs = None;
        MFCreateAttributes(&mut attrs, 3).map_err(err("io", "cannot read the video"))?;
        let attrs = attrs.ok_or_else(|| VideoError::new("io", "cannot read the video"))?;
        let set = match gpu {
            // The advanced processor is the one that runs on a device.
            Some(gpu) => attrs
                .SetUnknown(&MF_SOURCE_READER_D3D_MANAGER, gpu)
                .and_then(|()| attrs.SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1))
                .and_then(|()| {
                    attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)
                }),
            None => attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1),
        };
        set.map_err(err("io", "cannot read the video"))?;
        let reader = MFCreateSourceReaderFromByteStream(&bytestream, &attrs)
            .map_err(err("unsupported", "not a video Rux can read"))?;

        reader
            .SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)
            .ok();
        reader
            .SetStreamSelection(VIDEO, true)
            .map_err(|_| VideoError::new("unsupported", "it has no picture in it"))?;

        let ty = MFCreateMediaType().map_err(err("io", "cannot read the video"))?;
        ty.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video).ok();
        ty.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32).ok();
        reader.SetCurrentMediaType(VIDEO, None, &ty).map_err(|e| {
            if e.code() == MF_E_TOPO_CODEC_NOT_FOUND || e.code() == MF_E_INVALIDMEDIATYPE {
                VideoError::new(
                    "unsupported",
                    "its picture is in a format this machine cannot decode (a store extension may add it)",
                )
            } else {
                err("unsupported", "its picture cannot be decoded")(e)
            }
        })?;
        if gpu.is_some() {
            no_rate_conversion(&reader);
        }
        Ok(reader)
    }
}

/// The advanced processor converts to the frame rate the container claims,
/// and a WebM claims none: Media Foundation guesses 5 fps and dropped every
/// other frame of a 10 fps clip. Switched off on the processor itself, which
/// the reader put in the video stream's chain.
fn no_rate_conversion(reader: &IMFSourceReader) {
    let Ok(ex) = reader.cast::<IMFSourceReaderEx>() else {
        return;
    };
    for i in 0.. {
        let (mut category, mut transform) = (windows::core::GUID::zeroed(), None);
        if unsafe { ex.GetTransformForStream(VIDEO, i, Some(&mut category), &mut transform) }
            .is_err()
        {
            return;
        }
        if category == MFT_CATEGORY_VIDEO_PROCESSOR {
            if let Some(attrs) = transform.and_then(|t| unsafe { t.GetAttributes() }.ok()) {
                let _ = unsafe { attrs.SetUINT32(&MF_XVP_DISABLE_FRC, 1) };
            }
        }
    }
}

impl Decoder {
    pub fn open(input: Input, gpu: bool) -> Result<Decoder, VideoError> {
        start()?;
        let Input::Bytes { bytes, hint } = input;
        let on_gpu = gpu
            .then(hardware)
            .flatten()
            .and_then(|m| reader(&bytes, hint.as_deref(), Some(&m)).ok());
        let reader = match on_gpu {
            Some(r) => r,
            None => reader(&bytes, hint.as_deref(), None)?,
        };
        unsafe {
            let has_audio = reader.GetNativeMediaType(AUDIO, 0).is_ok();
            let duration = reader
                .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
                .ok()
                .and_then(|v: PROPVARIANT| u64::try_from(&v).ok())
                .filter(|&t| t > 0)
                .map(|t| t as f64 / TICKS);
            let mut d = Decoder {
                reader,
                width: 0,
                height: 0,
                left: 0,
                top: 0,
                stride: 0,
                duration,
                has_audio,
                seeking: None,
            };
            d.read_type()?;
            Ok(d)
        }
    }

    /// The size and layout of what the reader now hands out: read at open,
    /// and again whenever a stream says its type changed.
    fn read_type(&mut self) -> Result<(), VideoError> {
        unsafe {
            let ty = self
                .reader
                .GetCurrentMediaType(VIDEO)
                .map_err(err("decode", "its picture has no size"))?;
            let size = ty
                .GetUINT64(&MF_MT_FRAME_SIZE)
                .map_err(err("decode", "its picture has no size"))?;
            let (w, h) = ((size >> 32) as u32, size as u32);
            let mut area = MFVideoArea::default();
            let blob = std::slice::from_raw_parts_mut(
                &mut area as *mut MFVideoArea as *mut u8,
                std::mem::size_of::<MFVideoArea>(),
            );
            let shown = ty
                .GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, blob, None)
                .is_ok()
                && area.Area.cx > 0
                && area.Area.cy > 0;
            if shown {
                self.left = area.OffsetX.value.max(0) as u32;
                self.top = area.OffsetY.value.max(0) as u32;
                self.width = (area.Area.cx as u32).min(w - self.left.min(w));
                self.height = (area.Area.cy as u32).min(h - self.top.min(h));
            } else {
                (self.left, self.top, self.width, self.height) = (0, 0, w, h);
            }
            self.stride = ty
                .GetUINT32(&MF_MT_DEFAULT_STRIDE)
                .map(|s| s as i32)
                .unwrap_or(w as i32 * 4);
        }
        Ok(())
    }

    pub fn info(&self) -> Info {
        Info {
            width: self.width,
            height: self.height,
            duration: self.duration,
            has_audio: self.has_audio,
        }
    }

    pub fn next_frame(&mut self) -> Result<Option<Frame>, VideoError> {
        loop {
            let mut flags = 0u32;
            let mut ticks = 0i64;
            let mut sample = None;
            unsafe {
                self.reader
                    .ReadSample(
                        VIDEO,
                        0,
                        None,
                        Some(&mut flags),
                        Some(&mut ticks),
                        Some(&mut sample),
                    )
                    .map_err(err("decode", "its picture stopped decoding"))?;
            }
            let has = |f: MF_SOURCE_READER_FLAG| flags & f.0 as u32 != 0;
            if has(MF_SOURCE_READERF_ERROR) {
                return Err(VideoError::new("decode", "its picture stopped decoding"));
            }
            if has(MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED) {
                self.read_type()?;
            }
            let Some(sample) = sample else {
                if has(MF_SOURCE_READERF_ENDOFSTREAM) {
                    self.seeking = None;
                    return Ok(None);
                }
                // A gap in the stream (`STREAMTICK`): nothing to show yet.
                continue;
            };
            let time = ticks as f64 / TICKS;
            if let Some(target) = self.seeking {
                // A seek lands on the key frame before its target; the
                // frames between are decoded and passed over, unconverted,
                // up to the one on screen at the target.
                let lasts = unsafe { sample.GetSampleDuration() }
                    .map(|d| d as f64 / TICKS)
                    .unwrap_or(0.0);
                if time + lasts <= target + 1e-4 {
                    continue;
                }
                self.seeking = None;
            }
            return self.convert(&sample, time).map(Some);
        }
    }

    /// The sample's shown picture as RGBA, top row first. Media Foundation's
    /// RGB32 is B, G, R and an unused byte.
    fn convert(&self, sample: &IMFSample, time: f64) -> Result<Frame, VideoError> {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut rgba = vec![0u8; w * h * 4];
        unsafe {
            let buffer = sample
                .ConvertToContiguousBuffer()
                .map_err(err("decode", "a picture did not decode"))?;
            // A 2D buffer knows its own pitch, negative when it is stored
            // bottom row first; a plain one is laid out as the type says.
            let two_d = buffer.cast::<IMF2DBuffer>().ok();
            let mut base = std::ptr::null_mut();
            let mut pitch = self.stride;
            let mut len = 0u32;
            match &two_d {
                Some(b) => b
                    .Lock2D(&mut base, &mut pitch)
                    .map_err(err("decode", "a picture did not decode"))?,
                None => {
                    buffer
                        .Lock(&mut base, None, Some(&mut len))
                        .map_err(err("decode", "a picture did not decode"))?;
                    let needed = (self.top as usize + h).saturating_sub(1)
                        * pitch.unsigned_abs() as usize
                        + (self.left as usize + w) * 4;
                    if (len as usize) < needed {
                        buffer.Unlock().ok();
                        return Err(VideoError::new(
                            "decode",
                            "a picture came out smaller than its size",
                        ));
                    }
                    if pitch < 0 {
                        // Bottom-up: the first row shown is the buffer's last.
                        let rows = len as isize / -pitch as isize;
                        base = base.offset((rows - 1) * -pitch as isize);
                    }
                }
            }
            let rows_in = |y: usize| {
                base.offset(
                    (y + self.top as usize) as isize * pitch as isize + self.left as isize * 4,
                )
            };
            for y in 0..h {
                let src = std::slice::from_raw_parts(rows_in(y), w * 4);
                let dst = &mut rgba[y * w * 4..(y + 1) * w * 4];
                for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                    d.copy_from_slice(&[s[2], s[1], s[0], 255]);
                }
            }
            match &two_d {
                Some(b) => b.Unlock2D().ok(),
                None => buffer.Unlock().ok(),
            };
        }
        Ok(Frame {
            time,
            width: self.width,
            height: self.height,
            rgba,
        })
    }

    pub fn seek(&mut self, secs: f64) -> Result<(), VideoError> {
        let secs = match self.duration {
            Some(d) => secs.clamp(0.0, d),
            None => secs.max(0.0),
        };
        let at = PROPVARIANT::from((secs * TICKS) as i64);
        unsafe {
            self.reader
                .SetCurrentPosition(&windows::core::GUID::zeroed(), &at)
                .map_err(err("decode", "cannot seek in it"))?;
        }
        self.seeking = Some(secs);
        Ok(())
    }
}
