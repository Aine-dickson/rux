//! `<audio>` (`docs/13-file-input-and-media-plan.md`, B.3): the sound each
//! element plays, kept here under the element's key, and the events it fires.
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
    seen: bool,
}

impl Entry {
    fn new(src: &str) -> Entry {
        Entry {
            src: src.to_string(),
            load: Load::Idle,
            looping: None,
            muted: None,
            autoplayed: false,
            play_when_ready: false,
            playing: false,
            reported: 0.0,
            error: None,
            seen: true,
        }
    }
}

static SOUNDS: Mutex<Option<HashMap<String, Entry>>> = Mutex::new(None);
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

/// One of the `Sound` calls Rux's own module has.
fn call(key: &str, args: Vec<Any>) -> Result<Any, Error> {
    match registry::call(rux_runtime::builtins::BUILTIN_MODULE, key) {
        Some(Call::Sync(f)) => f(args),
        _ => Err(Error::new("unsupported", "this build has no sound")),
    }
}

fn on(h: &Handle, key: &str) -> Any {
    call(key, vec![Any::Resource(h.clone())]).unwrap_or(Any::None)
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
    if let Err(e) = call(what, all) {
        rux_runtime::warn_script(format!("an `<audio>` could not {}: {}", what.trim_start_matches("Sound."), e.message));
    }
}

/// Start reading `src` for the element keyed `key`, on the thread an
/// `async` native call runs on; the answer is kept and the window woken.
fn start_load(key: &str, e: &mut Entry) {
    let Some(Call::Async(f)) = registry::call(rux_runtime::builtins::BUILTIN_MODULE, "loadSound") else {
        e.error = Some(Error::new("unsupported", "this build has no sound"));
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
                        e.error = Some(Error::new("type", "`loadSound` gave something that is not a `Sound`"));
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
        act(h, "Sound.stop", Vec::new());
    }
}

/// Do what was asked of the element keyed `key`.
pub(crate) fn ask(key: &str, what: MediaAsk) {
    let mut guard = sounds();
    let Some(e) = guard.as_mut().and_then(|m| m.get_mut(key)) else { return };
    // Pressing play is the person's say-so, so nothing waits on `autoplay`.
    e.autoplayed = true;
    match &e.load {
        Load::Ready(h) => {
            let playing = matches!(on(h, "Sound.playing"), Any::Bool(true));
            let play = match what {
                MediaAsk::Play => true,
                MediaAsk::Pause => false,
                MediaAsk::Toggle => !playing,
            };
            act(h, if play { "Sound.play" } else { "Sound.pause" }, Vec::new());
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

/// Every `<audio>` in the tree, with the instance its handlers run in.
fn walk<'a>(node: &'a LayoutNode, out: &mut Vec<(&'a Media, Option<&'a str>)>) {
    if let Some(m) = node.field().media.as_deref() {
        out.push((m, node.instance.as_deref()));
    }
    for c in &node.children {
        walk(c, out);
    }
}

/// Bring the sounds in step with the `<audio>` elements in `root`: load the
/// new ones, let the gone ones go, apply `loop` and `muted`, decide
/// `autoplay`, and say what each sound did since it was last looked at.
/// Gives the handlers to run, with their instance and `event`.
pub(crate) fn sync(root: &LayoutNode) -> Vec<(String, Option<String>, Value)> {
    let mut found = Vec::new();
    walk(root, &mut found);
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
        for (m, instance) in found {
            let e = map.entry(m.key.clone()).or_insert_with(|| Entry::new(&m.src));
            e.seen = true;
            // A new `src` is a new sound, as a browser has it.
            if e.src != m.src {
                gone.push(std::mem::replace(e, Entry::new(&m.src)));
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
            if e.looping != Some(m.looping) {
                act(&h, "Sound.loop=", vec![Any::Bool(m.looping)]);
                e.looping = Some(m.looping);
            }
            if e.muted != Some(m.muted) {
                // `muted` is the volume at nothing, which the mute button
                // reads; unmuted is the volume as recorded.
                act(&h, "Sound.volume=", vec![Any::Float(if m.muted { 0.0 } else { 1.0 })]);
                e.muted = Some(m.muted);
            }
            if e.play_when_ready {
                e.play_when_ready = false;
                e.autoplayed = true;
                act(&h, "Sound.play", Vec::new());
            } else if !e.autoplayed {
                e.autoplayed = true;
                if m.autoplay {
                    if m.muted || ACTIVATED.load(Ordering::Relaxed) {
                        act(&h, "Sound.play", Vec::new());
                    } else {
                        rux_runtime::warn_script(format!(
                            "`<audio autoplay src=\"{}\">` did not play: a sound plays by itself only \
                             when it is `muted` or once the person has pressed something, as a browser \
                             has it. Add `muted`, or play it from a tap",
                            m.src
                        ));
                    }
                }
            }
            let playing = matches!(on(&h, "Sound.playing"), Any::Bool(true));
            let time = number(on(&h, "Sound.currentTime")).unwrap_or(0.0);
            let duration = number(on(&h, "Sound.duration"));
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
    }
    // Outside the lock: stopping a sound may wake the window.
    for e in gone {
        forget(e);
    }
    out
}

fn record(fields: &[(&str, Value)]) -> Value {
    Value::Map(fields.iter().map(|(k, v)| (k.to_string(), v.clone())).collect())
}
