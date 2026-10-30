//! `<audio>` (`docs/13-file-input-and-media-plan.md`, B.3): the sound each
//! element plays, kept here under the element's key, and the events it fires.
//! `<video>` (C.2) is the same with a `Video` in place of the `Sound`
//! (`crate::video`): every call is made on the handle's own type, so one
//! path drives both.
//!
//! **One code path on every platform.** A sound is loaded and played through
//! Rux's own module's calls, the same `loadSound` and `Sound` a script has
//! (`crate::sound` on the desktop and Android, `crate::web_sound` in a
//! browser), so an element is exactly what a script could have built by hand.
//! Its drawn controls reach the same `Sound` through `__rux_media` and the
//! element's key ([`lookup`]).
//!
//! **Events are found, not sent.** After every wake the shell looks at each
//! element's sound ([`sync`]): playing that was not is `@play`, the other way
//! round `@pause`, and at its end `@ended` after it, as a browser orders
//! them. So a play from the controls, from `query().play()`, from `autoplay`
//! and from a script holding the same `Sound` all fire the same events, and
//! nothing that plays a sound has to remember to.
//!
//! **`autoplay` follows the browser's rule everywhere**: a sound plays by
//! itself only when it is `muted`, or once the person has pressed something
//! in the app ([`activate`]). A desktop app that plays and a web build of it
//! that does not would be a worse surprise than one rule.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use rux_layout::{Media, Node as LayoutNode};
use rux_reactive::Value;
use rux_runtime::native::{registry, Any, Call, Error, Handle};
use rux_runtime::MediaAsk;

/// How often `@timeupdate` fires while a sound plays: four times a second,
/// as a browser does.
const TIME_STEP: f64 = 0.25;

enum Load {
    /// Not asked for yet (`preload="none"`, or no `src`).
    Idle,
    /// Being read; the number says which load, so an old one that finishes
    /// late is thrown away.
    Loading(u64),
    Ready(Handle),
    Failed,
}

struct Entry {
    src: String,
    /// A `<video>`, loaded with `loadVideo`.
    video: bool,
    /// `@loadedmetadata` has been given.
    met: bool,
    load: Load,
    /// What `loop` and `muted` were last given to the sound as.
    looping: Option<bool>,
    muted: Option<bool>,
    /// `autoplay` has been decided, one way or the other.
    autoplayed: bool,
    /// A play asked for before the sound was there.
    play_when_ready: bool,
    /// What the sound was doing when last looked at.
    playing: bool,
    /// The `currentTime` the last `@timeupdate` gave.
    reported: f64,
    /// Why it could not be loaded, until `@error` has been given it.
    error: Option<Error>,
    /// A video's own failure part way has been given as `@error`.
    failed_once: bool,
    seen: bool,
    /// Where the element is in the tree, for what the pointer is over.
    path: Vec<usize>,
}

impl Entry {
    fn new(src: &str, video: bool) -> Entry {
        Entry {
            src: src.to_string(),
            video,
            met: false,
            load: Load::Idle,
            looping: None,
            muted: None,
            autoplayed: false,
            play_when_ready: false,
            playing: false,
            reported: 0.0,
            error: None,
            failed_once: false,
            seen: true,
            path: Vec::new(),
        }
    }
}

static SOUNDS: Mutex<Option<HashMap<String, Entry>>> = Mutex::new(None);
/// The key of the `<video>` filling the window, if one does.
static FULLSCREEN: Mutex<Option<String>> = Mutex::new(None);
static NEXT_LOAD: AtomicU64 = AtomicU64::new(1);
static ACTIVATED: AtomicBool = AtomicBool::new(false);

fn sounds() -> std::sync::MutexGuard<'static, Option<HashMap<String, Entry>>> {
    SOUNDS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Give `__rux_media` its lookup.
pub(crate) fn install() {
    rux_runtime::builtins::set_media(lookup);
}

/// The sound of the `<audio>` keyed `key`, or nothing while it loads.
fn lookup(key: &str) -> Any {
    match sounds().as_ref().and_then(|m| m.get(key)).map(|e| &e.load) {
        Some(Load::Ready(h)) => Any::Resource(h.clone()),
        _ => Any::None,
    }
}

/// The person pressed something: from now on `autoplay` may play.
pub(crate) fn activate() {
    ACTIVATED.store(true, Ordering::Relaxed);
}

/// One of the `Sound` or `Video` calls Rux's own module has.
fn call(key: &str, args: Vec<Any>) -> Result<Any, Error> {
    match registry::call(rux_runtime::builtins::BUILTIN_MODULE, key) {
        Some(Call::Sync(f)) => f(args),
        _ => Err(Error::new("unsupported", "this build cannot play it")),
    }
}

/// `what` of the sound or video `h` is: `Sound.playing`, `Video.playing`.
fn named(h: &Handle, what: &str) -> String {
    format!("{}.{what}", h.type_name())
}

fn on(h: &Handle, what: &str) -> Any {
    call(&named(h, what), vec![Any::Resource(h.clone())]).unwrap_or(Any::None)
}

fn number(a: Any) -> Option<f64> {
    match a {
        Any::Float(f) => Some(f),
        Any::Int(i) => Some(i as f64),
        _ => None,
    }
}

/// Do `what` to a sound, saying on the console what went wrong.
fn act(h: &Handle, what: &str, args: Vec<Any>) {
    let mut all = vec![Any::Resource(h.clone())];
    all.extend(args);
    if let Err(e) = call(&named(h, what), all) {
        let tag = if h.type_name() == "Video" { "a `<video>`" } else { "an `<audio>`" };
        rux_runtime::warn_script(format!("{tag} could not {}: {}", what.trim_end_matches('='), e.message));
    }
}

/// Start reading `src` for the element keyed `key`, on the thread an
/// `async` native call runs on; the answer is kept and the window woken.
fn start_load(key: &str, e: &mut Entry) {
    let load = if e.video { "loadVideo" } else { "loadSound" };
    let Some(Call::Async(f)) = registry::call(rux_runtime::builtins::BUILTIN_MODULE, load) else {
        let what = if e.video { "video" } else { "sound" };
        e.error = Some(Error::new("unsupported", format!("this build plays no {what}")));
        e.load = Load::Failed;
        return;
    };
    let ticket = NEXT_LOAD.fetch_add(1, Ordering::Relaxed);
    e.load = Load::Loading(ticket);
    // Called here, on the document's thread, which is where a project
    // file's bytes and a picked file's URL are read.
    let reading = f(vec![Any::Str(e.src.clone())]);
    let key = key.to_string();
    rux_runtime::native::spawn(Box::pin(async move {
        let answer = reading.await;
        if let Some(e) = sounds().as_mut().and_then(|m| m.get_mut(&key)) {
            if matches!(e.load, Load::Loading(t) if t == ticket) {
                match answer {
                    Ok(Any::Resource(h)) => e.load = Load::Ready(h),
                    Ok(_) => {
                        e.error = Some(Error::new("type", format!("`{load}` gave something that is not playable")));
                        e.load = Load::Failed;
                    }
                    Err(err) => {
                        e.error = Some(err);
                        e.load = Load::Failed;
                    }
                }
            }
        }
        rux_runtime::native::notify();
    }));
}

/// Let a sound go: it stops first, since a `Sound` still held elsewhere (a
/// voice in the mixer) would otherwise play on with no element.
fn forget(e: Entry) {
    if let Load::Ready(h) = &e.load {
        act(h, "stop", Vec::new());
    }
}

/// Do what was asked of the element keyed `key`.
pub(crate) fn ask(key: &str, what: MediaAsk) {
    let mut guard = sounds();
    let Some(e) = guard.as_mut().and_then(|m| m.get_mut(key)) else { return };
    if what == MediaAsk::Fullscreen {
        if e.video {
            let on = fullscreen().as_deref() != Some(key);
            drop(guard);
            set_fullscreen(on.then_some(key));
        }
        return;
    }
    // Pressing play is the person's say-so, so nothing waits on `autoplay`.
    e.autoplayed = true;
    match &e.load {
        Load::Ready(h) => {
            let playing = matches!(on(h, "playing"), Any::Bool(true));
            let play = match what {
                MediaAsk::Play => true,
                MediaAsk::Pause => false,
                MediaAsk::Toggle | MediaAsk::Fullscreen => !playing,
            };
            act(h, if play { "play" } else { "pause" }, Vec::new());
        }
        // Not there yet: played once it is, which for `preload="none"` is
        // once this asks for it.
        Load::Idle | Load::Loading(_) => {
            let play = !matches!(what, MediaAsk::Pause);
            e.play_when_ready = play;
            if play && matches!(e.load, Load::Idle) && !e.src.is_empty() {
                start_load(key, e);
            }
        }
        Load::Failed => {}
    }
}

/// Every `<audio>` and `<video>` in the tree, with the instance its
/// handlers run in and its tree path.
fn walk<'a>(node: &'a LayoutNode, path: &mut Vec<usize>, out: &mut Vec<(&'a Media, Option<&'a str>, Vec<usize>)>) {
    if let Some(m) = node.field().media.as_deref() {
        out.push((m, node.instance.as_deref(), path.clone()));
    }
    for (i, c) in node.children.iter().enumerate() {
        path.push(i);
        walk(c, path, out);
        path.pop();
    }
}

/// The key of the `<video>` filling the window.
pub(crate) fn fullscreen() -> Option<String> {
    FULLSCREEN.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Put the `<video>` keyed `key` in fullscreen, or none: the one there before
/// leaves it. The shell follows with the window ([`fullscreen`]).
pub(crate) fn set_fullscreen(key: Option<&str>) {
    let before = std::mem::replace(&mut *FULLSCREEN.lock().unwrap_or_else(|e| e.into_inner()), key.map(str::to_string));
    for (k, h) in video_handles() {
        if Some(k.as_str()) == before.as_deref() || Some(k.as_str()) == key {
            act(&h, "fullscreen=", vec![Any::Bool(Some(k.as_str()) == key)]);
        }
    }
    rux_runtime::native::notify();
}

/// Leave fullscreen, if a video is in it: Escape, or Back on Android. Says
/// whether one was.
pub(crate) fn leave_fullscreen() -> bool {
    let was = fullscreen().is_some();
    if was {
        set_fullscreen(None);
    }
    was
}

/// The pointer is at `(x, y)`, moved or pressed there: each video under it
/// is stirred, and its controls come up a while. `metrics` is where every
/// node was laid out.
pub(crate) fn stir_at(metrics: &[rux_layout::NodeMetrics], x: f32, y: f32) {
    stir(|path| {
        metrics
            .iter()
            .find(|m| *m.path == path[..])
            .is_some_and(|m| x >= m.x && x <= m.x + m.width && y >= m.y && y <= m.y + m.height)
    });
}

/// A key reached what is focused at `focus`: a video it is inside is stirred.
pub(crate) fn stir_focused(focus: &[usize]) {
    stir(|path| focus.starts_with(path));
}

/// Each loaded `<video>` whose tree path `hit` takes is stirred: its
/// controls come up a while (`Video.stir`).
fn stir(hit: impl Fn(&[usize]) -> bool) {
    let handles: Vec<Handle> = {
        let guard = sounds();
        let Some(map) = guard.as_ref() else { return };
        map.values()
            .filter(|e| e.video && hit(&e.path))
            .filter_map(|e| match &e.load {
                Load::Ready(h) => Some(h.clone()),
                _ => None,
            })
            .collect()
    };
    for h in handles {
        act(&h, "stir", Vec::new());
    }
}

/// Bring the sounds in step with the `<audio>` elements in `root`: load the
/// new ones, let the gone ones go, apply `loop` and `muted`, decide
/// `autoplay`, and say what each sound did since it was last looked at.
/// Gives the handlers to run, with their instance and `event`.
pub(crate) fn sync(root: &LayoutNode) -> Vec<(String, Option<String>, Value)> {
    let mut found = Vec::new();
    walk(root, &mut Vec::new(), &mut found);
    let mut out = Vec::new();
    let mut gone = Vec::new();
    {
        let mut guard = sounds();
        let map = guard.get_or_insert_with(HashMap::new);
        if found.is_empty() && map.is_empty() {
            return out;
        }
        for e in map.values_mut() {
            e.seen = false;
        }
        for (m, instance, path) in found {
            let e = map.entry(m.key.clone()).or_insert_with(|| Entry::new(&m.src, m.video));
            e.seen = true;
            e.path = path;
            // A new `src` is a new sound, as a browser has it.
            if e.src != m.src || e.video != m.video {
                gone.push(std::mem::replace(e, Entry::new(&m.src, m.video)));
            }
            if matches!(e.load, Load::Idle) && !m.lazy && !e.src.is_empty() {
                start_load(&m.key, e);
            }
            let fire = |out: &mut Vec<_>, i: usize, event: Value| {
                if let Some(body) = &m.on[i] {
                    out.push((body.clone(), instance.map(str::to_string), event));
                }
            };
            if let Some(err) = e.error.take() {
                fire(&mut out, 4, record(&[("kind", Value::Text(err.kind)), ("message", Value::Text(err.message))]));
            }
            let Load::Ready(h) = &e.load else { continue };
            let h = h.clone();
            if !e.met {
                e.met = true;
                // A new `src` while fullscreen stays fullscreen.
                if e.video && fullscreen().as_deref() == Some(m.key.as_str()) {
                    act(&h, "fullscreen=", vec![Any::Bool(true)]);
                }
                // Its pictures play without it, as in a browser, and the
                // console says why it is silent.
                if e.video {
                    if let Any::Str(why) = on(&h, "soundError") {
                        rux_runtime::warn_script(format!("`<video src=\"{}\">` plays without its sound: {why}", e.src));
                    }
                }
                let size = |what| match on(&h, what) {
                    Any::Int(n) => n,
                    _ => 0,
                };
                fire(
                    &mut out,
                    5,
                    record(&[
                        ("duration", number(on(&h, "duration")).map_or(Value::Null, Value::Number)),
                        ("videoWidth", Value::Number(size("videoWidth") as f64)),
                        ("videoHeight", Value::Number(size("videoHeight") as f64)),
                    ]),
                );
            }
            // A video that stopped decoding part way says so once.
            if let Some(Any::Str(err)) = e.video.then(|| on(&h, "error")) {
                let (kind, message) = err.split_once('\t').unwrap_or(("decode", &err));
                if e.error.is_none() && !e.failed_once {
                    e.failed_once = true;
                    fire(&mut out, 4, record(&[("kind", Value::Text(kind.into())), ("message", Value::Text(message.into()))]));
                }
            }
            if e.looping != Some(m.looping) {
                act(&h, "loop=", vec![Any::Bool(m.looping)]);
                e.looping = Some(m.looping);
            }
            if e.muted != Some(m.muted) {
                // `muted` is the volume at nothing, which the mute button
                // reads; unmuted is the volume as recorded.
                act(&h, "volume=", vec![Any::Float(if m.muted { 0.0 } else { 1.0 })]);
                e.muted = Some(m.muted);
            }
            if e.play_when_ready {
                e.play_when_ready = false;
                e.autoplayed = true;
                act(&h, "play", Vec::new());
            } else if !e.autoplayed {
                e.autoplayed = true;
                if m.autoplay {
                    if m.muted || ACTIVATED.load(Ordering::Relaxed) {
                        act(&h, "play", Vec::new());
                    } else {
                        rux_runtime::warn_script(format!(
                            "`<{} autoplay src=\"{}\">` did not play: `autoplay` starts only when \
                             it is `muted` or once the person has pressed something in the app. \
                             Add `muted`, or call `.play()` from a tap handler",
                            if m.video { "video" } else { "audio" },
                            m.src
                        ));
                    }
                }
            }
            let playing = matches!(on(&h, "playing"), Any::Bool(true));
            let time = number(on(&h, "currentTime")).unwrap_or(0.0);
            let duration = number(on(&h, "duration"));
            let now = || {
                record(&[
                    ("currentTime", Value::Number(time)),
                    ("duration", duration.map_or(Value::Null, Value::Number)),
                ])
            };
            if (time - e.reported).abs() >= TIME_STEP || (playing != e.playing && time != e.reported) {
                e.reported = time;
                fire(&mut out, 3, now());
            }
            if playing != e.playing {
                e.playing = playing;
                fire(&mut out, if playing { 0 } else { 1 }, now());
                // At its end rather than paused: the sound has no end flag
                // every platform shares, so a stop within a frame of the
                // length is the end. One of unknown length only pauses.
                if !playing && duration.is_some_and(|d| time >= d - 0.05) {
                    fire(&mut out, 2, now());
                }
            }
        }
        let keys: Vec<String> = map.iter().filter(|(_, e)| !e.seen).map(|(k, _)| k.clone()).collect();
        for k in keys {
            gone.extend(map.remove(&k));
        }
        // A fullscreen video gone from the page leaves the window as it was.
        if fullscreen().is_some_and(|k| !map.contains_key(&k)) {
            *FULLSCREEN.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }
    // Outside the lock: stopping a sound may wake the window.
    for e in gone {
        forget(e);
    }
    out
}

/// Every `<video>` playing now, by key, for the frame's upload.
/// Every loaded `<video>`'s `Video`, by key.
pub(crate) fn video_handles() -> Vec<(String, Handle)> {
    let guard = sounds();
    let Some(map) = guard.as_ref() else { return Vec::new() };
    map.iter()
        .filter(|(_, e)| e.video)
        .filter_map(|(k, e)| match &e.load {
            Load::Ready(h) => Some((k.clone(), h.clone())),
            _ => None,
        })
        .collect()
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn videos() -> Vec<(String, std::sync::Arc<crate::video::Player>)> {
    let guard = sounds();
    let Some(map) = guard.as_ref() else { return Vec::new() };
    map.iter()
        .filter_map(|(k, e)| match &e.load {
            Load::Ready(h) => crate::video::player(h).map(|p| (k.clone(), p)),
            _ => None,
        })
        .collect()
}

fn record(fields: &[(&str, Value)]) -> Value {
    Value::Map(fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect())
}
