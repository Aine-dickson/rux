//! `<video>`'s player (`docs/13-file-input-and-media-plan.md`, C.2): a
//! thread per video that decodes ahead with `rux-video` and puts up each
//! picture when its time comes, and the `Video` resource the element's
//! controls and `query()` reach it by, shaped as a `Sound` is.
//!
//! **The sound is the clock** (C.3). A video's sound track is decoded by
//! the platform ([`rux_video::SoundTrack`]) and played through `rux-audio`'s
//! mixer as any `Sound` is, and the pictures follow where the mixer has got
//! to: the player keeps its own clock (playing from `t` at instant `i`, the
//! time is `t + (now - i)`) and sets it to the sound's whenever the two are
//! more than [`SLACK`] apart. So the pictures wait while the output opens,
//! and a sound clock that runs a little fast or slow takes them with it. A
//! video with no sound, or one this machine cannot decode, runs on the wall's
//! clock alone. A picture is put up once the clock reaches it and passed over
//! when the one after it is due too, so a slow decode drops pictures rather
//! than falling behind.
//!
//! **Out of sight, a video plays on and draws nothing**: the painter says
//! which pictures it drew ([`after_scene`]), and one it did not draw is not
//! written into its texture or asked a redraw for, while its sound plays on.
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
use rux_video::{Decoder, Frame, Info, SoundTrack};

/// How often a playing video tells the document its time moved, for the
/// controls and `@timeupdate`.
const TICK: Duration = Duration::from_millis(100);

/// How long a playing video's controls stay up after it was last stirred:
/// about what a browser's do.
const CONTROLS_IDLE: Duration = Duration::from_secs(3);

/// How far the pictures' clock may be from the sound's before it is set to
/// it: more than the sound clock's own jitter between the mixer's steps, and
/// well under the 45 ms by which a picture ahead of its sound starts to show.
const SLACK: f64 = 0.02;

/// Within this of the sound, the pictures' clock is drawn to it a tenth of
/// the gap at a time rather than set: unnoticed, and it leaves no lasting
/// offset of up to [`SLACK`] after the last time it was set.
const NUDGE: f64 = 0.002;

/// How long before a picture is due the thread stops waiting on its condvar
/// and sleeps the rest: more than the system tick it would otherwise land on.
const PRECISE: f64 = 0.02;

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
        self.0.room.notify_all();
    }
}

pub(crate) struct Player {
    info: Info,
    /// Its sound, played through the mixer; `None` without one.
    sound: Option<rux_audio::Sound>,
    /// Why its sound does not play, for a warning: the video plays on
    /// without it, as a browser's does.
    sound_error: Option<rux_video::VideoError>,
    /// The painter drew its picture in the last frame.
    shown: AtomicBool,
    /// When the pointer, a tap or a key last stirred it: its controls stay
    /// up for [`CONTROLS_IDLE`] after.
    stirred: Mutex<Instant>,
    /// It fills the window ([`crate::media`] decides which one does).
    fullscreen: AtomicBool,
    state: Mutex<State>,
    /// Rung whenever the state changes, for the threads waiting on it.
    wake: Condvar,
    pictures: Mutex<Pictures>,
    /// Rung when a picture is added or taken, a seek is asked for, or the
    /// video goes.
    room: Condvar,
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
    /// The sound was started with the pictures and is expected to be
    /// playing: its clock is followed, and it stopping by anything but its
    /// end pauses the pictures too.
    with_sound: bool,
    /// How many times the sound had ended when it was started.
    sound_ended: u64,
}

impl Player {
    /// Taken before [`Player::pictures`] when both are.
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn pictures(&self) -> MutexGuard<'_, Pictures> {
        self.pictures.lock().unwrap_or_else(|e| e.into_inner())
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
        self.start_sound(&mut s);
        drop(s);
        // Up for a while as it starts, then out of the way.
        *self.stirred.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
        crate::sound::refocus();
        self.wake.notify_all();
        rux_runtime::native::notify();
    }

    /// Start the sound with the pictures, from where they are. A sound that
    /// will not start (no output) leaves the pictures on the wall's clock.
    fn start_sound(&self, s: &mut State) {
        let Some(sound) = &self.sound else { return };
        let at = Self::current_time(s, &self.info);
        if (sound.current_time() - at).abs() > SLACK {
            sound.seek(at);
        }
        s.with_sound = sound.play().is_ok();
        s.sound_ended = sound.ended();
    }

    /// Bring the pictures' clock to the sound's, or pause them with it.
    fn follow_sound(&self, s: &mut State) {
        let Some(sound) = self.sound.as_ref().filter(|_| s.with_sound && s.playing) else { return };
        if sound.playing() {
            let heard = sound.heard_time();
            let gap = heard - Self::current_time(s, &self.info);
            if gap.abs() > SLACK {
                s.position = heard;
                s.since = Some(Instant::now());
            } else if gap.abs() > NUDGE {
                s.position += gap / 10.0;
            }
        } else if sound.ended() != s.sound_ended || sound.error().is_some() {
            // The sound is shorter than the pictures, or failed: they go on
            // by the wall's clock.
            s.with_sound = false;
        } else {
            // Paused from outside: the app was put away, another app took
            // the sound, the output went. The pictures pause with it.
            s.position = Self::current_time(s, &self.info);
            s.playing = false;
            s.since = None;
            s.with_sound = false;
            rux_runtime::native::notify();
        }
    }

    pub(crate) fn pause(&self) {
        let mut s = self.state();
        s.position = Self::current_time(&s, &self.info);
        s.playing = false;
        s.since = None;
        s.with_sound = false;
        drop(s);
        if let Some(sound) = &self.sound {
            sound.pause();
        }
        crate::sound::refocus();
        self.wake.notify_all();
        rux_runtime::native::notify();
    }

    fn set_volume(&self, v: f64) {
        self.state().volume = v;
        if let Some(sound) = &self.sound {
            sound.set_volume(v as f32);
        }
        crate::sound::refocus();
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
        if let Some(sound) = &self.sound {
            sound.seek(secs);
        }
        self.wake.notify_all();
        self.room.notify_all();
        rux_runtime::native::notify();
    }

    /// Whether its drawn controls are up: always while paused, and while
    /// playing for [`CONTROLS_IDLE`] after it was last stirred.
    fn controls_shown(&self) -> bool {
        !self.state().playing || self.stirred.lock().unwrap_or_else(|e| e.into_inner()).elapsed() < CONTROLS_IDLE
    }

    /// The pointer moved over it, it was pressed, or a key reached it: its
    /// controls come up, and stay a while.
    pub(crate) fn stir(&self) {
        let hidden = !self.controls_shown();
        *self.stirred.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
        if hidden {
            rux_runtime::native::notify();
        }
    }

    pub(crate) fn set_fullscreen(&self, on: bool) {
        if self.fullscreen.swap(on, Ordering::AcqRel) != on {
            self.stir();
            rux_runtime::native::notify();
        }
    }

    /// The picture showing now, and its number.
    pub(crate) fn frame(&self) -> Option<(Arc<Frame>, u64)> {
        let s = self.state();
        s.frame.clone().map(|f| (f, s.serial))
    }
}

/// How many pictures are kept decoded ahead of the one showing: room for a
/// slow decode without a missed frame. At 1080p60 a decode took 7.6 ms at
/// the median and up to 60 ms, and with one frame ahead 3 to 5 in a hundred
/// were missed.
const AHEAD: usize = 4;

/// Pictures decoded and not yet shown: filled by the decoding thread,
/// emptied by the clock's.
struct Pictures {
    queue: VecDeque<Frame>,
    /// The decoder has no more: at its end, or failed.
    done: bool,
}

/// The next picture, or `None` at the end. A picture stamped at or past the
/// length the file states is past its end: the clock stops at that length,
/// so it would never come due.
fn next(player: &Player, decoder: &mut Decoder) -> Option<Frame> {
    match decoder.next_frame() {
        Ok(Some(f)) if !player.info.duration.is_some_and(|d| f.time >= d - 1e-3) => Some(f),
        Ok(_) => None,
        Err(e) => {
            // What was decoded still plays; then it ends, as at its end.
            fail(player, e);
            None
        }
    }
}

/// The decoding thread: keep [`AHEAD`] pictures decoded, and make seeks.
///
/// **Decoding and showing are two threads.** As one, a thread that waits
/// for a picture's time decodes at most one picture between two shown, so
/// at 1080p60 (a 7.6 ms decode in a 16.7 ms frame) it never caught up after
/// a slow one, and 2 to 4 pictures in a hundred were missed.
fn decode(player: Arc<Player>, mut decoder: Decoder) {
    // The first picture is up at once, as a browser shows a video's first
    // frame before it plays.
    match next(&player, &mut decoder) {
        Some(f) => put_up(&player, f),
        None => player.pictures().done = true,
    }
    loop {
        let mut s = player.state();
        if s.quit {
            return;
        }
        if let Some(t) = s.seek.take() {
            // Emptied under the state's lock, so the clock never sees a
            // picture from before the seek once it is taken.
            let mut p = player.pictures();
            p.queue.clear();
            p.done = false;
            drop(p);
            drop(s);
            match decoder.seek(t) {
                // The picture at the new time is up at once, playing or not.
                Ok(()) => match next(&player, &mut decoder) {
                    Some(f) => put_up(&player, f),
                    None => player.pictures().done = true,
                },
                Err(e) => {
                    player.pictures().done = true;
                    fail(&player, e);
                }
            }
            player.wake.notify_all();
            continue;
        }
        drop(s);
        let p = player.pictures();
        if p.done || p.queue.len() >= AHEAD {
            // Until a picture is taken, a seek is asked for, or it goes.
            let _unused = player.room.wait_timeout(p, Duration::from_millis(250));
            continue;
        }
        drop(p);
        let f = next(&player, &mut decoder);
        // One decoded while a seek was asked for is from before it: the
        // clock waits while a seek is pending, and the seek empties it.
        let mut p = player.pictures();
        match f {
            Some(f) => p.queue.push_back(f),
            None => p.done = true,
        }
        drop(p);
        player.room.notify_all();
    }
}

/// The clock's thread: put each picture up when its time comes, follow the
/// sound, and end or go round at the end.
fn keep_time(player: Arc<Player>) {
    let mut told = Instant::now();
    loop {
        let mut s = player.state();
        if s.quit {
            return;
        }
        if !s.playing || s.seek.is_some() {
            let _unused = player.wake.wait_timeout(s, Duration::from_millis(250));
            continue;
        }
        player.follow_sound(&mut s);
        if !s.playing {
            continue;
        }
        let now = Player::current_time(&s, &player.info);
        if told.elapsed() >= TICK {
            told = Instant::now();
            rux_runtime::native::notify();
        }
        let mut p = player.pictures();
        let Some(due) = p.queue.front().map(|f| f.time) else {
            if !p.done {
                // The decoder is behind: woken by its next picture.
                drop(s);
                let _unused = player.room.wait_timeout(p, Duration::from_millis(50));
                continue;
            }
            drop(p);
            // The end: around again, or stopped there, the sound with it.
            if s.looping {
                s.seek = Some(0.0);
                s.position = 0.0;
                s.since = Some(Instant::now());
                if let Some(sound) = &player.sound {
                    sound.seek(0.0);
                }
                player.start_sound(&mut s);
                drop(s);
                player.room.notify_all();
            } else {
                s.playing = false;
                s.at_end = true;
                s.position = player.info.duration.unwrap_or(now);
                s.since = None;
                s.with_sound = false;
                drop(s);
                if let Some(sound) = &player.sound {
                    sound.pause();
                }
                crate::sound::refocus();
                rux_runtime::native::notify();
            }
            continue;
        };
        if due > now {
            drop(p);
            let wait = due - now;
            if wait > PRECISE {
                // Until nearly due, or a pause or a seek.
                let wait = Duration::from_secs_f64((wait - PRECISE).min(0.25));
                let _unused = player.wake.wait_timeout(s, wait);
            } else {
                // The last stretch is slept, not waited on the condvar,
                // whose wait on Windows lands on the 15.6 ms system tick:
                // every picture went up 8 to 25 ms late, and 1 to 3 in a
                // hundred too late to show. A pause or seek in this short
                // while is seen at its end.
                drop(s);
                std::thread::sleep(Duration::from_secs_f64(wait));
            }
            continue;
        }
        let frame = p.queue.pop_front().expect("looked at above");
        let after = p.queue.front().map(|n| n.time);
        drop(p);
        drop(s);
        player.room.notify_all();
        // Due: up at once. One so late that the picture after it is due too
        // is passed over, so a decoder that fell behind catches up rather
        // than showing the past.
        let lasts = after.map(|n| n - frame.time).filter(|d| *d > 0.0).unwrap_or(1.0 / 30.0);
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
    if player.shown.load(Ordering::Acquire) {
        wake_for_frame();
    }
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

/// A video's sound track as a `Sound` the mixer plays, read from the same
/// bytes as its pictures.
fn sound_of(input: rux_video::Input, duration: Option<f64>) -> Result<rux_audio::Sound, rux_video::VideoError> {
    let open = move || -> Result<Box<dyn rux_audio::Stream>, rux_audio::AudioError> {
        Ok(Box::new(Track(SoundTrack::open(input.clone()).map_err(heard)?)))
    };
    rux_audio::Sound::from_stream(open, duration)
        .map_err(|e| rux_video::VideoError { kind: e.kind, message: e.message })
}

fn heard(e: rux_video::VideoError) -> rux_audio::AudioError {
    rux_audio::AudioError::new(e.kind, e.message)
}

/// A [`SoundTrack`] as the mixer reads a stream.
struct Track(SoundTrack);

impl rux_audio::Stream for Track {
    fn rate(&self) -> u32 {
        self.0.rate()
    }

    fn channels(&self) -> usize {
        self.0.channels()
    }

    fn next(&mut self) -> Result<Option<Vec<f32>>, rux_audio::AudioError> {
        self.0.next_samples().map_err(heard)
    }

    fn seek(&mut self, secs: f64) -> Result<(), rux_audio::AudioError> {
        self.0.seek(secs).map_err(heard)
    }
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
                        .spawn(move || match Decoder::open(input.clone()) {
                            Ok(decoder) => {
                                let (sound, sound_error) = match decoder.info().has_audio {
                                    true => match sound_of(input, decoder.info().duration) {
                                        Ok(s) => (Some(s), None),
                                        Err(e) => (None, Some(e)),
                                    },
                                    false => (None, None),
                                };
                                let player = Arc::new(Player {
                                    info: decoder.info().clone(),
                                    sound,
                                    sound_error,
                                    shown: AtomicBool::new(true),
                                    stirred: Mutex::new(Instant::now()),
                                    fullscreen: AtomicBool::new(false),
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
                                        with_sound: false,
                                        sound_ended: 0,
                                    }),
                                    wake: Condvar::new(),
                                    pictures: Mutex::new(Pictures { queue: VecDeque::with_capacity(AHEAD), done: false }),
                                    room: Condvar::new(),
                                });
                                let clock = player.clone();
                                if let Err(e) = std::thread::Builder::new()
                                    .name("rux-video-clock".into())
                                    .spawn(move || keep_time(clock))
                                {
                                    let _ = tx.send(Err(Error::new("io", format!("cannot start a video: {e}"))));
                                    return;
                                }
                                let _ = tx.send(Ok(player.clone()));
                                decode(player, decoder);
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
        ("Video.controlsShown".into(), get(|p| Any::Bool(p.controls_shown()))),
        ("Video.fullscreen".into(), get(|p| Any::Bool(p.fullscreen.load(Ordering::Acquire)))),
        (
            "Video.error".into(),
            get(|p| match p.state().error.clone() {
                Some(e) => Any::Str(format!("{}\t{}", e.kind, e.message)),
                None => Any::None,
            }),
        ),
        (
            "Video.soundError".into(),
            get(|p| p.sound_error.as_ref().map_or(Any::None, |e| Any::Str(e.message.clone()))),
        ),
        (
            "Video.volume=".into(),
            set(|p, v| {
                p.set_volume(number(Some(v))?.clamp(0.0, 1.0));
                Ok(())
            }),
        ),
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
            // Out of sight: written in only once, so it has a picture when it
            // comes back.
            if !player.shown.load(Ordering::Acquire) && on.contains_key(key) {
                continue;
            }
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

/// After a frame's scene is built: each video learns whether the painter drew
/// it. One that came into sight with a newer picture than it drew asks for
/// another frame at once.
pub(crate) fn after_scene(players: &[(String, Arc<Player>)], images: &mut rux_paint::ImageCache) {
    let drawn = images.take_frames_drawn();
    for (key, player) in players {
        let shown = drawn.contains(&format!("{}{key}", rux_layout::VIDEO_FRAME));
        let was = player.shown.swap(shown, Ordering::AcqRel);
        let behind = || {
            let serial = player.state().serial;
            ON_GPU.with(|on| on.borrow().get(key).is_none_or(|g| g.serial != serial))
        };
        if shown && !was && behind() {
            wake_for_frame();
        }
    }
}

/// Pause every video, as when the app is put away.
#[cfg(target_os = "android")]
pub(crate) fn pause_all(players: &[(String, Arc<Player>)]) {
    for (_, p) in players {
        p.pause();
    }
}

/// The player behind a `Video` handle.
pub(crate) fn player(h: &Handle) -> Option<Arc<Player>> {
    h.get::<VideoRes>().map(|v| v.0.clone())
}
