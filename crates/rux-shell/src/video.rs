//! `<video>`'s player (`docs/13-file-input-and-media-plan.md`, C.2): a
//! thread per video that decodes ahead with `rux-video` and puts up each
//! picture when its time comes, and the `Video` resource the element's
//! controls and `query()` reach it by, shaped as a `Sound` is.
//!
//! **The clock is the wall's until C.3** gives the video its sound: playing
//! from `t` at instant `i`, the time is `t + (now - i)`. A picture is put up
//! once the clock reaches it and passed over when the one after it is due
//! too, so a slow decode drops pictures rather than falling behind.
//!
//! **Pictures reach the screen by C.0's texture road** ([`upload`]): one
//! texture per video, registered with vello, each new picture written into
//! it, and the painter drawing it as an `<image>` whose `src` is
//! [`rux_layout::VIDEO_FRAME`] and the element's key. A new picture asks only
//! for a redraw ([`set_frame_wake`]), never a rebuild of the document.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use rux_runtime::native::{Any, Call, Error, Handle, Resource};
use rux_video::{Decoder, Frame, Info};

/// How often a playing video tells the document its time moved, for the
/// controls and `@timeupdate`.
const TICK: Duration = Duration::from_millis(100);

/// A loaded video, as Rux holds it. When the last of it goes, its thread
/// stops.
pub(crate) struct VideoRes(Arc<Player>);

impl Resource for VideoRes {
    const NAME: &'static str = "Video";
}

impl Drop for VideoRes {
    fn drop(&mut self) {
        self.0.state().quit = true;
        self.0.wake.notify_all();
    }
}

pub(crate) struct Player {
    info: Info,
    state: Mutex<State>,
    /// Rung whenever the state changes, for the thread waiting on it.
    wake: Condvar,
}

struct State {
    playing: bool,
    looping: bool,
    volume: f64,
    /// Where it is while paused; where it was at [`State::since`] while
    /// playing.
    position: f64,
    since: Option<Instant>,
    /// A seek the thread has not made yet.
    seek: Option<f64>,
    /// It played to its end, and the next `play` starts it again.
    at_end: bool,
    /// The picture to show, and how many have been put up, so the uploader
    /// knows a new one.
    frame: Option<Arc<Frame>>,
    serial: u64,
    /// Played or sought at least once: the poster gives way to the picture.
    started: bool,
    error: Option<rux_video::VideoError>,
    quit: bool,
}

impl Player {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn current_time(s: &State, info: &Info) -> f64 {
        let t = match s.since {
            Some(i) => s.position + i.elapsed().as_secs_f64(),
            None => s.position,
        };
        info.duration.map_or(t, |d| t.min(d))
    }

    pub(crate) fn time(&self) -> f64 {
        Self::current_time(&self.state(), &self.info)
    }

    pub(crate) fn play(&self) {
        let mut s = self.state();
        if s.at_end {
            s.at_end = false;
            s.position = 0.0;
            s.seek = Some(0.0);
        }
        if !s.playing {
            s.playing = true;
            s.since = Some(Instant::now());
        }
        s.started = true;
        drop(s);
        self.wake.notify_all();
        rux_runtime::native::notify();
    }

    pub(crate) fn pause(&self) {
        let mut s = self.state();
        s.position = Self::current_time(&s, &self.info);
        s.playing = false;
        s.since = None;
        drop(s);
        self.wake.notify_all();
        rux_runtime::native::notify();
    }

    pub(crate) fn seek(&self, secs: f64) {
        let secs = self.info.duration.map_or(secs.max(0.0), |d| secs.clamp(0.0, d));
        let mut s = self.state();
        s.position = secs;
        s.seek = Some(secs);
        s.at_end = false;
        s.started = true;
        if s.playing {
            s.since = Some(Instant::now());
        }
        drop(s);
        self.wake.notify_all();
        rux_runtime::native::notify();
    }

    /// The picture showing now, and its number.
    pub(crate) fn frame(&self) -> Option<(Arc<Frame>, u64)> {
        let s = self.state();
        s.frame.clone().map(|f| (f, s.serial))
    }
}

/// How many pictures the thread keeps decoded ahead of the one showing:
/// room for a slow decode without a missed frame. At 1080p60 a decode took
/// 7.6 ms at the median and up to 60 ms, and with one frame ahead 3 to 5 in
/// a hundred were missed.
const AHEAD: usize = 4;

/// Decode the next picture into `queue`, or say there are no more. A picture
/// stamped at or past the length the file states is past its end: the clock
/// stops at that length, so it would never come due.
fn fill(player: &Player, decoder: &mut Decoder, queue: &mut VecDeque<Frame>, done: &mut bool) {
    match decoder.next_frame() {
        Ok(Some(f)) if !player.info.duration.is_some_and(|d| f.time >= d - 1e-3) => queue.push_back(f),
        Ok(_) => *done = true,
        Err(e) => {
            // What was decoded still plays; then it ends, as at its end.
            *done = true;
            fail(player, e);
        }
    }
}

/// The decoding thread: keep a few pictures decoded ahead, and put each up
/// when the clock reaches it.
fn run(player: Arc<Player>, mut decoder: Decoder) {
    let mut queue = VecDeque::with_capacity(AHEAD);
    let mut done = false;
    // The first picture is up at once, as a browser shows a video's first
    // frame before it plays.
    fill(&player, &mut decoder, &mut queue, &mut done);
    if let Some(f) = queue.pop_front() {
        put_up(&player, f);
    }
    let mut told = Instant::now();
    loop {
        let mut s = player.state();
        if s.quit {
            return;
        }
        if let Some(t) = s.seek.take() {
            drop(s);
            queue.clear();
            done = false;
            if let Err(e) = decoder.seek(t) {
                done = true;
                fail(&player, e);
                continue;
            }
            // The picture at the new time is up at once, playing or not.
            fill(&player, &mut decoder, &mut queue, &mut done);
            if let Some(f) = queue.pop_front() {
                put_up(&player, f);
            }
            continue;
        }
        let room = queue.len() < AHEAD && !done;
        if !s.playing {
            if room {
                drop(s);
                fill(&player, &mut decoder, &mut queue, &mut done);
            } else {
                let _unused = player.wake.wait(s);
            }
            continue;
        }
        let now = Player::current_time(&s, &player.info);
        if told.elapsed() >= TICK {
            told = Instant::now();
            rux_runtime::native::notify();
        }
        let Some(front) = queue.front() else {
            if !done {
                drop(s);
                fill(&player, &mut decoder, &mut queue, &mut done);
                continue;
            }
            // The end: around again, or stopped there.
            if s.looping {
                s.seek = Some(0.0);
                s.position = 0.0;
                s.since = Some(Instant::now());
            } else {
                s.playing = false;
                s.at_end = true;
                s.position = player.info.duration.unwrap_or(now);
                s.since = None;
                drop(s);
                rux_runtime::native::notify();
            }
            continue;
        };
        if front.time > now {
            // Not yet: decode ahead while there is room, else wait for it,
            // or for a pause or a seek.
            if room {
                drop(s);
                fill(&player, &mut decoder, &mut queue, &mut done);
            } else {
                let wait = Duration::from_secs_f64((front.time - now).min(0.25));
                let _unused = player.wake.wait_timeout(s, wait);
            }
            continue;
        }
        drop(s);
        // Due: up at once. One so late that the picture after it is due too
        // is passed over, so a decoder that fell behind catches up rather
        // than showing the past.
        let frame = queue.pop_front().expect("looked at above");
        let lasts = queue.front().map(|n| n.time - frame.time).filter(|d| *d > 0.0).unwrap_or(1.0 / 30.0);
        if frame.time + lasts > player.time() {
            put_up(&player, frame);
        }
    }
}

fn put_up(player: &Player, frame: Frame) {
    let mut s = player.state();
    s.frame = Some(Arc::new(frame));
    s.serial += 1;
    drop(s);
    wake_for_frame();
}

fn fail(player: &Player, e: rux_video::VideoError) {
    player.state().error = Some(e);
    rux_runtime::native::notify();
}

type Wake = Arc<dyn Fn() + Send + Sync>;
static FRAME_WAKE: Mutex<Option<Wake>> = Mutex::new(None);
/// A redraw has been asked for and not yet drawn: one is enough.
static FRAME_ASKED: AtomicBool = AtomicBool::new(false);

/// How a new picture asks for a redraw, from the decoding thread.
pub(crate) fn set_frame_wake(f: impl Fn() + Send + Sync + 'static) {
    *FRAME_WAKE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(f));
}

fn wake_for_frame() {
    if FRAME_ASKED.swap(true, Ordering::AcqRel) {
        return;
    }
    let wake = FRAME_WAKE.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(wake) = wake {
        wake();
    }
}

/// The video a call's first argument is.
fn video(args: &[Any]) -> Result<Arc<VideoRes>, Error> {
    match args.first() {
        Some(Any::Resource(h)) => h.get::<VideoRes>().ok_or_else(|| Error::new("type", "this is not a `Video`")),
        _ => Err(Error::new("type", "this is not a `Video`")),
    }
}

fn number(v: Option<&Any>) -> Result<f64, Error> {
    match v {
        Some(Any::Float(f)) => Ok(*f),
        Some(Any::Int(i)) => Ok(*i as f64),
        _ => Err(Error::new("type", "a number")),
    }
}

/// What `src` holds, read on the script's thread, as a sound's is.
fn input(src: &str) -> Result<rux_video::Input, Error> {
    #[cfg(target_os = "android")]
    if src.starts_with("content://") {
        let bytes = crate::android_read_uri(src).ok_or_else(|| Error::new("io", format!("cannot read {src}")))?;
        return Ok(rux_video::Input::Bytes { bytes: bytes.into(), hint: None });
    }
    let bytes = rux_runtime::read_media(src).map_err(|e| Error::new("io", format!("cannot read {src}: {e}")))?;
    let hint = std::path::Path::new(src.split(['#', '?']).next().unwrap_or(src))
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase());
    Ok(rux_video::Input::Bytes { bytes: bytes.into(), hint })
}

/// The calls behind `<video>`, keyed as the module's are. `loadVideo` is the
/// element's alone: no script names it, as the module does not declare it.
pub(crate) fn calls() -> Vec<(String, Call)> {
    let get = |f: fn(&Player) -> Any| Call::sync(move |args| Ok(f(&video(&args)?.0)));
    let act = |f: fn(&Player)| {
        Call::sync(move |args| {
            f(&video(&args)?.0);
            Ok(Any::None)
        })
    };
    let set = |f: fn(&Player, &Any) -> Result<(), Error>| {
        Call::sync(move |args| {
            f(&video(&args)?.0, args.get(1).unwrap_or(&Any::None))?;
            rux_runtime::native::notify();
            Ok(Any::None)
        })
    };
    vec![
        (
            "loadVideo".into(),
            Call::future(|args: Vec<Any>| {
                let read = match args.first() {
                    Some(Any::Str(s)) => input(s),
                    _ => Err(Error::new("type", "`loadVideo` takes the video's path or URL")),
                };
                async move {
                    let (tx, rx) = std::sync::mpsc::channel();
                    let input = read?;
                    // Opened on the thread that will decode it: Media
                    // Foundation's reader belongs to the thread it is made on.
                    std::thread::Builder::new()
                        .name("rux-video".into())
                        .spawn(move || match Decoder::open(input) {
                            Ok(decoder) => {
                                let player = Arc::new(Player {
                                    info: decoder.info().clone(),
                                    state: Mutex::new(State {
                                        playing: false,
                                        looping: false,
                                        volume: 1.0,
                                        position: 0.0,
                                        since: None,
                                        seek: None,
                                        at_end: false,
                                        frame: None,
                                        serial: 0,
                                        started: false,
                                        error: None,
                                        quit: false,
                                    }),
                                    wake: Condvar::new(),
                                });
                                let _ = tx.send(Ok(player.clone()));
                                run(player, decoder);
                            }
                            Err(e) => {
                                let _ = tx.send(Err(Error::new(e.kind, e.message)));
                            }
                        })
                        .map_err(|e| Error::new("io", format!("cannot start a video: {e}")))?;
                    let player = rx.recv().map_err(|_| Error::new("decode", "the video stopped opening"))??;
                    Ok(Any::Resource(Handle::new(VideoRes(player))))
                }
            }),
        ),
        ("Video.play".into(), act(Player::play)),
        ("Video.pause".into(), act(Player::pause)),
        ("Video.stop".into(), act(|p| {
            p.pause();
            p.seek(0.0);
        })),
        (
            "Video.seek".into(),
            Call::sync(|args| {
                video(&args)?.0.seek(number(args.get(1))?);
                Ok(Any::None)
            }),
        ),
        ("Video.playing".into(), get(|p| Any::Bool(p.state().playing))),
        ("Video.currentTime".into(), get(|p| Any::Float(p.time()))),
        ("Video.duration".into(), get(|p| p.info.duration.map_or(Any::None, Any::Float))),
        ("Video.volume".into(), get(|p| Any::Float(p.state().volume))),
        ("Video.loop".into(), get(|p| Any::Bool(p.state().looping))),
        ("Video.videoWidth".into(), get(|p| Any::Int(i64::from(p.info.width)))),
        ("Video.videoHeight".into(), get(|p| Any::Int(i64::from(p.info.height)))),
        ("Video.started".into(), get(|p| Any::Bool(p.state().started))),
        (
            "Video.error".into(),
            get(|p| match p.state().error.clone() {
                Some(e) => Any::Str(format!("{}\t{}", e.kind, e.message)),
                None => Any::None,
            }),
        ),
        ("Video.volume=".into(), set(|p, v| Ok(p.state().volume = number(Some(v))?.clamp(0.0, 1.0)))),
        (
            "Video.loop=".into(),
            set(|p, v| match v {
                Any::Bool(b) => {
                    p.state().looping = *b;
                    p.wake.notify_all();
                    Ok(())
                }
                _ => Err(Error::new("type", "`true` or `false`")),
            }),
        ),
        (
            "Video.currentTime=".into(),
            set(|p, v| {
                p.seek(number(Some(v))?);
                Ok(())
            }),
        ),
    ]
}

/// A video's picture on the GPU: its texture, and the image vello draws it
/// by.
struct OnGpu {
    texture: vello::wgpu::Texture,
    image: vello::peniko::ImageData,
    serial: u64,
}

thread_local! {
    static ON_GPU: std::cell::RefCell<HashMap<String, OnGpu>> = std::cell::RefCell::new(HashMap::new());
}

/// Before a frame is drawn: write each video's newest picture into its
/// texture, and tell the painter what to draw for it. `players` is every
/// `<video>` on the page with its player, by key; one no longer there lets
/// its texture go.
pub(crate) fn upload(
    players: &[(String, Arc<Player>)],
    device: &vello::wgpu::Device,
    queue: &vello::wgpu::Queue,
    renderer: &mut vello::Renderer,
    images: &mut rux_paint::ImageCache,
) {
    use vello::wgpu;
    FRAME_ASKED.store(false, Ordering::Release);
    ON_GPU.with(|on| {
        let mut on = on.borrow_mut();
        on.retain(|key, gpu| {
            let keep = players.iter().any(|(k, _)| k == key);
            if !keep {
                renderer.unregister_texture(gpu.image.clone());
                images.set_frame(&format!("{}{key}", rux_layout::VIDEO_FRAME), None);
            }
            keep
        });
        for (key, player) in players {
            let Some((frame, serial)) = player.frame() else { continue };
            let size = wgpu::Extent3d { width: frame.width, height: frame.height, depth_or_array_layers: 1 };
            if let Some(gpu) = on.get(key) {
                if gpu.serial == serial {
                    continue;
                }
                // A new size (a stream that changed) is a new texture.
                if gpu.texture.size() != size {
                    let gone = on.remove(key).expect("there");
                    renderer.unregister_texture(gone.image);
                }
            }
            let gpu = on.entry(key.clone()).or_insert_with(|| {
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("rux: video"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let image = renderer.register_texture(texture.clone());
                OnGpu { texture, image, serial: 0 }
            });
            queue.write_texture(
                gpu.texture.as_image_copy(),
                &frame.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(frame.width * 4),
                    rows_per_image: Some(frame.height),
                },
                size,
            );
            renderer.mark_override_image_dirty(&gpu.image);
            gpu.serial = serial;
            images.set_frame(
                &format!("{}{key}", rux_layout::VIDEO_FRAME),
                Some(vello::peniko::ImageBrush::new(gpu.image.clone())),
            );
        }
    });
}

/// The player behind a `Video` handle.
pub(crate) fn player(h: &Handle) -> Option<Arc<Player>> {
    h.get::<VideoRes>().map(|v| v.0.clone())
}
